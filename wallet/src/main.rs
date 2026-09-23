mod cache;
use std::num::NonZeroU32;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use bip0039::{Count, English, Mnemonic};
use orchard::keys::{PreparedIncomingViewingKey, Scope};
use orchard::note_encryption::IronwoodDomain;
use rand::rngs::OsRng;
use zcash_note_encryption::try_note_decryption;
use zcash_client_backend::address::Address;
use zcash_client_backend::data_api::wallet::{
    create_proposed_transactions, decrypt_and_store_transaction,
    propose_standard_transfer_to_address, ConfirmationsPolicy, SpendingKeys,
};
use zcash_client_backend::wallet::OvkPolicy;
use zcash_client_backend::fees::StandardFeeRule;
use zcash_proofs::prover::LocalTxProver;
use zcash_protocol::value::Zatoshis;
use zcash_protocol::ShieldedProtocol;
use zcash_client_backend::data_api::TransactionDataRequest;
use zcash_client_backend::decrypt_transaction;
use zcash_client_backend::data_api::{AccountBirthday, AccountPurpose, WalletRead, WalletWrite};
use zcash_client_backend::keys::UnifiedAddressRequest;
use zcash_client_backend::proto::service::{
    compact_tx_streamer_client::CompactTxStreamerClient, BlockId, ChainSpec, RawTransaction,
    TxFilter,
};
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::{BlockHeight, BranchId};
use zcash_protocol::memo::{Memo, MemoBytes};
use zcash_client_backend::sync;
use zcash_client_sqlite::util::SystemClock;
use zcash_client_sqlite::wallet::init::init_wallet_db;
use zcash_client_sqlite::WalletDb;
use zcash_keys::keys::UnifiedFullViewingKey;

use crate::cache::MemoryBlockCache;
use zcash_keys::keys::UnifiedSpendingKey;
use zcash_protocol::consensus::Network;
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
fn generate(network: &Network) -> Result<()> {
    let mnemonic = <Mnemonic<English>>::generate(Count::Words24);
    let seed = mnemonic.to_seed("");

    let usk = UnifiedSpendingKey::from_seed(network, &seed, AccountId::ZERO)
        .context("deriving the spending key")?;
    let ufvk = usk.to_unified_full_viewing_key();
    let (address, _) = ufvk
        .default_address(UnifiedAddressRequest::AllAvailableKeys)
        .context("deriving the address")?;

    println!("seed phrase (24 words; write it on paper, never paste it anywhere):");
    println!("{}", mnemonic.phrase());
    println!();
    println!("viewing key (safe on the server; reads, cannot spend):");
    println!("{}", ufvk.encode(network));
    println!();
    println!("address (publish this; positions are sent here):");
    println!("{}", address.encode(network));

    Ok(())
}

/// Where to sync from, per network. Testnet exists so the whole pipeline can
/// be proved without moving money: the coins have no value and the faucet
/// gives them away.
fn endpoint(network: &Network) -> &'static str {
    match network {
        Network::MainNetwork => "https://zec.rocks:443",
        Network::TestNetwork => "https://testnet.zec.rocks:443",
    }
}

/// What a memo says, or why it says nothing.
fn describe(bytes: &MemoBytes) -> String {
    match Memo::try_from(bytes) {
        Ok(Memo::Text(text)) => format!("{:?}", text.to_string()),
        Ok(Memo::Empty) => "(empty)".to_string(),
        Ok(Memo::Future(_)) => "(a kind this build does not know)".to_string(),
        Ok(Memo::Arbitrary(_)) => "(not text)".to_string(),
        Err(_) => "(unreadable)".to_string(),
    }
}

/// Syncs the chain against a viewing key and reports what arrived.
///
/// The viewing key reads every payment and can spend none of them, which is
/// the most a process reachable from the internet should be trusted with.
async fn scan(network: &Network, ufvk_text: &str, birthday: u32, data: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&data).context("making the data directory")?;

    let ufvk = UnifiedFullViewingKey::decode(network, ufvk_text)
        .map_err(|e| anyhow!("that is not a viewing key for this network: {e}"))?;

    let mut client = CompactTxStreamerClient::connect(endpoint(network))
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

    let mut db = WalletDb::for_path(data.join("wallet.sqlite"), *network, SystemClock, OsRng)
        .context("opening the wallet database")?;
    init_wallet_db(&mut db, None).map_err(|e| anyhow!("preparing the wallet database: {e}"))?;

    if db.get_account_ids().map_err(|e| anyhow!("{e}"))?.is_empty() {
        db.import_account_ufvk("ring", &ufvk, &birthday, AccountPurpose::ViewOnly, None)
            .map_err(|e| anyhow!("importing the viewing key: {e}"))?;
        println!("imported the viewing key");
    }

    let cache = MemoryBlockCache::new();
    println!("chain tip {tip}, syncing...");
    sync::run(&mut client, network, &cache, &mut db, 1000)
        .await
        .map_err(|e| anyhow!("syncing: {e:?}"))?;

    // One confirmation: a position is counted once its block exists, and the
    // round it belongs to is the one after that, so waiting longer would only
    // count it late.
    // Scanning only reads compact blocks, which carry the first 52 bytes of a
    // note's ciphertext: enough to spot the note and read its value, not
    // enough to read its memo. Scanning queues up which transactions need
    // fetching in full; this answers those requests.
    let mut enhanced = 0usize;
    let mut received: Vec<(BlockHeight, u64, String)> = Vec::new();
    for request in db.transaction_data_requests().map_err(|e| anyhow!("{e}"))? {
        let TransactionDataRequest::Enhancement(txid) = request else {
            continue;
        };

        // TxId already holds the internal byte order the wire wants; it is
        // only *displayed* reversed. Reversing here asked the node about a
        // transaction that does not exist, and the node's error printed the
        // mirrored id back, which is how it was caught.
        let hash = txid.as_ref().to_vec();
        let raw = match client
            .get_transaction(TxFilter { block: None, index: 0, hash })
            .await
        {
            Ok(response) => response.into_inner(),
            // One transaction the server will not hand over should not end
            // the scan; the rest of the wallet is still worth reading.
            Err(status) => {
                eprintln!("  could not fetch {txid}: {}", status.message());
                continue;
            }
        };
        if raw.data.is_empty() {
            continue;
        }

        let height = BlockHeight::from_u32(u32::try_from(raw.height).unwrap_or(0));
        let branch = BranchId::for_height(network, height);
        let tx = Transaction::read(&raw.data[..], branch)
            .with_context(|| format!("reading transaction {txid}"))?;

        // Read it before storing it: the stored form keeps the note but the
        // memo is only handed back here, and the memo is the whole point.
        let ufvks = db.get_unified_full_viewing_keys().map_err(|e| anyhow!("{e}"))?;
        let decrypted = decrypt_transaction(network, Some(height), None, &tx, &ufvks);
        for output in decrypted.sapling_outputs() {
            received.push((height, u64::from(output.note_value()), describe(output.memo())));
        }
        for output in decrypted.orchard_outputs() {
            // An Orchard decrypted note is the note beside the pool it came
            // from, so the value is one step further in than Sapling's.
            let (note, _pool) = output.note();
            received.push((height, note.value().inner(), describe(output.memo())));
        }

        // Ironwood is where a wallet puts money now, and this is where the
        // library stops: DecryptedTransaction offers sapling_outputs() and
        // orchard_outputs() and nothing for this pool, though the balance
        // side counts it. Scanning found the note; only the memo was missing.
        // The bundle is structurally an Orchard one, but its notes are a
        // different plaintext version, so it needs IronwoodDomain rather than
        // OrchardDomain. Trying the wrong one raises nothing at all — trial
        // decryption just returns None — so it reads as "no memo" forever.
        if let Some(bundle) = tx.ironwood_bundle() {
            for ufvk in ufvks.values() {
                let Some(fvk) = ufvk.orchard() else { continue };
                let ivk = PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::External));
                for action in bundle.actions() {
                    let domain = IronwoodDomain::for_action(action);
                    if let Some((note, _address, memo)) =
                        try_note_decryption(&domain, &ivk, action)
                    {
                        received.push((
                            height,
                            note.value().inner(),
                            describe(&MemoBytes::from_bytes(&memo).unwrap_or_else(|_| MemoBytes::empty())),
                        ));
                    }
                }
            }
        }

        decrypt_and_store_transaction(network, &mut db, &tx, Some(height))
            .map_err(|e| anyhow!("storing transaction {txid}: {e}"))?;
        enhanced += 1;
    }
    if enhanced > 0 {
        println!("fetched {enhanced} transaction(s) in full");
    }

    let policy = ConfirmationsPolicy::new_symmetrical(NonZeroU32::MIN);
    let summary = db.get_wallet_summary(policy).map_err(|e| anyhow!("{e}"))?;
    // What arrived, and what it said. The memo is the whole reason for the
    // round trip: it is where a position names what it is backing.
    for (height, value, memo) in &received {
        println!("  received {value} zatoshi in block {}  memo {memo}", u32::from(*height));
    }

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

    // Testnet is not a default. A key made on the wrong network cannot
    // receive what was sent to it, and the addresses look similar enough to
    // be pasted in the wrong place.
    let network = if args.iter().any(|a| a == "--testnet") {
        Network::TestNetwork
    } else {
        Network::MainNetwork
    };

    // The subcommand is wherever it is: `--testnet new` and `new --testnet`
    // both have to work, and reading argv[0] made the flag shadow it.
    let command = args
        .iter()
        .find(|a| matches!(a.as_str(), "new" | "scan" | "send"))
        .map(String::as_str);

    match command {
        Some("new") => generate(&network),
        Some("scan") => {
            let ufvk = flag("--ufvk").ok_or_else(|| anyhow!("--ufvk is required"))?;
            let birthday: u32 = flag("--birthday")
                .ok_or_else(|| anyhow!("--birthday is required: the height the wallet was made"))?
                .parse()
                .context("--birthday must be a block height")?;
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            scan(&network, &ufvk, birthday, PathBuf::from(data)).await
        }
        Some("send") => {
            let seed = flag("--seed-file").ok_or_else(|| anyhow!("--seed-file is required"))?;
            let to = flag("--to").ok_or_else(|| anyhow!("--to is required"))?;
            let zatoshi: u64 = flag("--zatoshi")
                .ok_or_else(|| anyhow!("--zatoshi is required"))?
                .parse()
                .context("--zatoshi must be a whole number of zatoshi")?;
            let memo = flag("--memo").unwrap_or_default();
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            send(&network, PathBuf::from(seed), &to, zatoshi, &memo, PathBuf::from(data)).await
        }
        _ => bail!(
            "usage:\n  ring-wallet [--testnet] new\n  ring-wallet [--testnet] scan --ufvk <key> --birthday <height> [--data <dir>]\n  ring-wallet [--testnet] send --seed-file <path> --to <addr> --zatoshi <n> [--memo <text>] [--data <dir>]"
        ),
    }
}

/// Spends from the wallet, with a memo.
///
/// This needs the spending key, which the scanner never sees. It is read from
/// a file holding the seed phrase and nothing else: the point of the split is
/// that the process which is reachable from the internet cannot do this.
async fn send(
    network: &Network,
    seed_file: PathBuf,
    to_text: &str,
    zatoshi: u64,
    memo_text: &str,
    data: PathBuf,
) -> Result<()> {
    let phrase = std::fs::read_to_string(&seed_file)
        .with_context(|| format!("reading {}", seed_file.display()))?;
    let mnemonic = <Mnemonic<English>>::from_phrase(phrase.trim())
        .map_err(|e| anyhow!("that file does not hold a seed phrase: {e}"))?;
    let seed = mnemonic.to_seed("");
    let usk = UnifiedSpendingKey::from_seed(network, &seed, AccountId::ZERO)
        .context("deriving the spending key")?;

    let to = Address::decode(network, to_text)
        .ok_or_else(|| anyhow!("that is not an address for this network"))?;
    let amount = Zatoshis::from_u64(zatoshi).context("that amount is not a value")?;
    let memo = MemoBytes::from_bytes(memo_text.as_bytes())
        .map_err(|_| anyhow!("a memo is at most 512 bytes"))?;

    let mut db = WalletDb::for_path(data.join("wallet.sqlite"), *network, SystemClock, OsRng)
        .context("opening the wallet database")?;
    let account = *db
        .get_account_ids()
        .map_err(|e| anyhow!("{e}"))?
        .first()
        .ok_or_else(|| anyhow!("this wallet has no account; run scan first"))?;

    let proposal = propose_standard_transfer_to_address::<_, _, std::convert::Infallible>(
        &mut db,
        network,
        StandardFeeRule::Zip317,
        account,
        ConfirmationsPolicy::new_symmetrical(NonZeroU32::MIN),
        &to,
        amount,
        Some(memo),
        None,
        ShieldedProtocol::Orchard,
        None,
        None,
    )
    .map_err(|e| anyhow!("planning the payment: {e:?}"))?;

    println!("planned: {} output(s), proving...", proposal.steps().len());

    // Sapling proving needs two parameter files, about fifty megabytes, and
    // the API asks for the provers whether or not any Sapling note is spent.
    // Fetched once and cached where every other Zcash tool looks for them.
    let prover = match LocalTxProver::with_default_location() {
        Some(prover) => prover,
        None => {
            println!("fetching the Sapling parameters (about 50 MB, once)...");
            zcash_proofs::download_parameters()
                .map_err(|e| anyhow!("downloading the Sapling parameters: {e}"))?;
            LocalTxProver::with_default_location()
                .ok_or_else(|| anyhow!("the parameters downloaded but could not be loaded"))?
        }
    };

    let txids = create_proposed_transactions::<_, _, std::convert::Infallible, _, std::convert::Infallible, _>(
        &mut db,
        network,
        &prover,
        &prover,
        &SpendingKeys::from_unified_spending_key(usk),
        OvkPolicy::Sender,
        &proposal,
        None,
    )
    .map_err(|e| anyhow!("building the payment: {e:?}"))?;

    // Creating a transaction stores it; it does not broadcast it. There is no
    // send_authorized_transactions here — that helper belongs to another
    // library — so the raw bytes are read back out and handed to the network.
    let mut client = CompactTxStreamerClient::connect(endpoint(network))
        .await
        .context("reaching lightwalletd")?;

    for txid in &txids {
        let tx = db
            .get_transaction(*txid)
            .map_err(|e| anyhow!("{e}"))?
            .ok_or_else(|| anyhow!("the wallet built {txid} and then lost it"))?;
        let mut raw = Vec::new();
        tx.write(&mut raw).context("serialising the transaction")?;

        let response = client
            .send_transaction(RawTransaction { data: raw, height: 0 })
            .await
            .with_context(|| format!("broadcasting {txid}"))?
            .into_inner();

        // The server answers with a code and a string, and a non-zero code is
        // a refusal however friendly the string reads.
        if response.error_code != 0 {
            bail!(
                "the network refused {txid}: code {} {}",
                response.error_code,
                response.error_message
            );
        }
        println!("sent {txid}");
    }

    Ok(())
}
