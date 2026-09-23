mod cache;
use std::num::NonZeroU32;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use bip0039::{Count, English, Mnemonic};
use rand::rngs::OsRng;
use zcash_client_backend::data_api::wallet::ConfirmationsPolicy;
use zcash_client_backend::data_api::{AccountBirthday, AccountPurpose, WalletRead, WalletWrite};
use zcash_client_backend::keys::UnifiedAddressRequest;
use zcash_client_backend::proto::service::{
    compact_tx_streamer_client::CompactTxStreamerClient, BlockId, ChainSpec,
};
use zcash_client_backend::sync;
use zcash_client_sqlite::util::SystemClock;
use zcash_client_sqlite::wallet::init::init_wallet_db;
use zcash_client_sqlite::WalletDb;
use zcash_keys::keys::UnifiedFullViewingKey;

use crate::cache::MemoryBlockCache;
use zcash_keys::keys::UnifiedSpendingKey;
use zcash_protocol::consensus::MAIN_NETWORK;
use zip32::AccountId;

/// Generates the wallet the market receives positions at.
///
/// It is deliberately not the treasury wallet. The treasury holds what the
/// pool earned; this one holds what other people staked, and the two must
/// never sit in the same place — not for tidiness, but because mixing them
/// makes it impossible to say afterwards whose money was whose.
///
/// Three things come out, and they are not interchangeable. The seed can
/// spend and never touches a server. The viewing key can read every position
/// and spend none of them, which is what a machine reachable from the
/// internet is allowed to hold. The address is published.
fn generate() -> Result<()> {
    let mnemonic = <Mnemonic<English>>::generate(Count::Words24);
    let seed = mnemonic.to_seed("");

    let usk = UnifiedSpendingKey::from_seed(&MAIN_NETWORK, &seed, AccountId::ZERO)
        .context("deriving the spending key")?;
    let ufvk = usk.to_unified_full_viewing_key();
    let (address, _) = ufvk
        .default_address(UnifiedAddressRequest::AllAvailableKeys)
        .context("deriving the address")?;

    println!("seed phrase (24 words; write it on paper, never paste it anywhere):");
    println!("{}", mnemonic.phrase());
    println!();
    println!("viewing key (safe on the server; reads, cannot spend):");
    println!("{}", ufvk.encode(&MAIN_NETWORK));
    println!();
    println!("address (publish this; positions are sent here):");
    println!("{}", address.encode(&MAIN_NETWORK));

    Ok(())
}

const LIGHTWALLETD: &str = "https://zec.rocks:443";

/// Syncs the chain against a viewing key and reports what arrived.
///
/// The viewing key reads every payment and can spend none of them, which is
/// the most a process reachable from the internet should be trusted with.
async fn scan(ufvk_text: &str, birthday: u32, data: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&data).context("making the data directory")?;

    let ufvk = UnifiedFullViewingKey::decode(&MAIN_NETWORK, ufvk_text)
        .map_err(|e| anyhow!("that is not a mainnet viewing key: {e}"))?;

    let mut client = CompactTxStreamerClient::connect(LIGHTWALLETD)
        .await
        .context("reaching lightwalletd")?;

    let tip = client
        .get_latest_block(ChainSpec {})
        .await
        .context("asking for the chain tip")?
        .into_inner()
        .height;

    // A birthday is the commitment tree as of the block *before* the wallet
    // starts, because a tree is only meaningful as a position within it.
    let treestate = client
        .get_tree_state(BlockId {
            height: u64::from(birthday.saturating_sub(1)),
            hash: vec![],
        })
        .await
        .context("asking for the tree state at the birthday")?
        .into_inner();
    let birthday = AccountBirthday::from_treestate(treestate, None)
        .map_err(|e| anyhow!("that birthday will not do: {e:?}"))?;

    let mut db = WalletDb::for_path(data.join("wallet.sqlite"), MAIN_NETWORK, SystemClock, OsRng)
        .context("opening the wallet database")?;
    init_wallet_db(&mut db, None).map_err(|e| anyhow!("preparing the wallet database: {e}"))?;

    if db.get_account_ids().map_err(|e| anyhow!("{e}"))?.is_empty() {
        db.import_account_ufvk("ring", &ufvk, &birthday, AccountPurpose::ViewOnly, None)
            .map_err(|e| anyhow!("importing the viewing key: {e}"))?;
        println!("imported the viewing key");
    }

    let cache = MemoryBlockCache::new();
    println!("chain tip {tip}, syncing...");
    sync::run(&mut client, &MAIN_NETWORK, &cache, &mut db, 1000)
        .await
        .map_err(|e| anyhow!("syncing: {e:?}"))?;

    // One confirmation: a position is counted once its block exists, and the
    // round it belongs to is the one after that, so waiting longer would only
    // count it late.
    let policy = ConfirmationsPolicy::new_symmetrical(NonZeroU32::MIN);
    let summary = db.get_wallet_summary(policy).map_err(|e| anyhow!("{e}"))?;
    match summary {
        None => println!("nothing scanned yet"),
        Some(summary) => {
            println!("scanned to {}", u32::from(summary.chain_tip_height()));
            for (id, account) in summary.account_balances() {
                println!("  account {id:?}: {} zatoshi", u64::from(account.total()));
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // rustls refuses to guess when more than one provider could be linked.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };

    match args.first().map(String::as_str) {
        Some("new") => generate(),
        Some("scan") => {
            let ufvk = flag("--ufvk").ok_or_else(|| anyhow!("--ufvk is required"))?;
            let birthday: u32 = flag("--birthday")
                .ok_or_else(|| anyhow!("--birthday is required: the height the wallet was made"))?
                .parse()
                .context("--birthday must be a block height")?;
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            scan(&ufvk, birthday, PathBuf::from(data)).await
        }
        _ => bail!("usage: ring-wallet new | ring-wallet scan --ufvk <key> --birthday <height> [--data <dir>]"),
    }
}
