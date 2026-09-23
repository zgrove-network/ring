mod cache;
mod ledger;
mod serve;
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
use zcash_keys::address::UnifiedAddress;
use zcash_keys::keys::UnifiedFullViewingKey;

use crate::cache::MemoryBlockCache;
use crate::ledger::{Deposit, Ledger};
use zcash_keys::keys::UnifiedSpendingKey;
use zcash_protocol::consensus::Network;
use zip32::{AccountId, DiversifierIndex};

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

/// One address per depositor, derived from the same viewing key.
///
/// Unified addresses carry a diversifier: one key produces a practically
/// unlimited supply of addresses, all spendable by the same wallet, and
/// nobody outside can tell that two of them belong together. So the address
/// itself can say who paid, without the payer having to write anything.
pub fn derive(ufvk: &UnifiedFullViewingKey, index: u32) -> Result<(UnifiedAddress, u32)> {
    let j = DiversifierIndex::from(index);
    let (address, found) = ufvk
        .find_address(j, UnifiedAddressRequest::AllAvailableKeys)
        .map_err(|e| anyhow!("deriving address {index}: {e}"))?;
    let bytes = *found.as_bytes();
    let mut at = 0u64;
    for (i, b) in bytes.iter().take(8).enumerate() {
        at |= u64::from(*b) << (8 * i);
    }
    Ok((address, u32::try_from(at).unwrap_or(u32::MAX)))
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
async fn scan(
    network: &Network,
    ufvk_text: &str,
    birthday: u32,
    addresses: u32,
    data: PathBuf,
) -> Result<()> {
    std::fs::create_dir_all(&data).context("making the data directory")?;

    // The wallet database is a cache of the chain; the ledger is the record.
    // Deposits are written from the queue of transactions the wallet asks to
    // have fetched in full, and that queue empties once they are fetched — so
    // a ledger that is behind its wallet can never catch up on its own, and
    // would sit there looking merely quiet. Rebuilding the cache is cheap;
    // a book that silently owes people less than it should is not.
    let ledger_path = data.join("ledger.sqlite");
    let wallet_path = data.join("wallet.sqlite");
    if wallet_path.exists() {
        let behind = {
            let ledger = Ledger::open(&ledger_path)?;
            ledger.is_empty()?
        };
        if behind {
            println!("the ledger is empty and the wallet is not; rebuilding from the birthday");
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", wallet_path.display()));
            }
        }
    }

    let ufvk = UnifiedFullViewingKey::decode(network, ufvk_text)
        .map_err(|e| anyhow!("that is not a viewing key for this network: {e}"))?;

    // Every address this key can hand out, so a note can be traced back to
    // the depositor it was meant for without the payer writing anything.
    let mut issued: Vec<([u8; 43], u32)> = Vec::new();
    for index in 0..addresses {
        // Keyed on the index the address actually came out at, which is what
        // a depositor was given. Keying on the requested one would file three
        // different depositors' money under three names for one address.
        let (address, actual) = derive(&ufvk, index)?;
        if let Some(orchard) = address.orchard() {
            if !issued.iter().any(|(_, i)| *i == actual) {
                issued.push((orchard.to_raw_address_bytes(), actual));
            }
        }
    }

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
    let mut received: Vec<Deposit> = Vec::new();
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
        for (index, output) in decrypted.sapling_outputs().iter().enumerate() {
            received.push(Deposit {
                txid: txid.to_string(),
                output: u32::try_from(index).unwrap_or(0),
                address_index: u32::MAX,
                zatoshi: u64::from(output.note_value()),
                height: u32::from(height),
                memo: describe(output.memo()),
            });
        }
        for (index, output) in decrypted.orchard_outputs().iter().enumerate() {
            // An Orchard decrypted note is the note beside the pool it came
            // from, so the value is one step further in than Sapling's.
            let (note, _pool) = output.note();
            received.push(Deposit {
                txid: txid.to_string(),
                output: u32::try_from(index).unwrap_or(0),
                address_index: u32::MAX,
                zatoshi: note.value().inner(),
                height: u32::from(height),
                memo: describe(output.memo()),
            });
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
                for (index, action) in bundle.actions().iter().enumerate() {
                    let domain = IronwoodDomain::for_action(action);
                    if let Some((note, address, memo)) =
                        try_note_decryption(&domain, &ivk, action)
                    {
                        let raw = address.to_raw_address_bytes();
                        let at = issued.iter().find(|(a, _)| *a == raw).map(|(_, i)| *i);
                        received.push(Deposit {
                            txid: txid.to_string(),
                            output: u32::try_from(index).unwrap_or(0),
                            address_index: at.unwrap_or(u32::MAX),
                            zatoshi: note.value().inner(),
                            height: u32::from(height),
                            memo: describe(
                                &MemoBytes::from_bytes(&memo).unwrap_or_else(|_| MemoBytes::empty()),
                            ),
                        });
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
    // Writing them down is what makes a rescan safe: the note is the key, so
    // reading the chain again credits nobody twice.
    let ledger = Ledger::open(&data.join("ledger.sqlite"))?;
    let mut fresh = 0usize;
    for deposit in &received {
        let who = if deposit.address_index == u32::MAX {
            "an address not in the issued range".to_string()
        } else {
            format!("address {}", deposit.address_index)
        };
        let new = ledger.record_deposit(deposit)?;
        if new {
            fresh += 1;
        }
        println!(
            "  {} {} zatoshi in block {}  at {who}  memo {}",
            if new { "recorded" } else { "already had" },
            deposit.zatoshi,
            deposit.height,
            deposit.memo
        );
    }
    if !received.is_empty() {
        println!("  {fresh} new, {} already known", received.len() - fresh);
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

#[cfg(test)]
mod tests {
    use super::*;
    use bip0039::{Count, English, Mnemonic};

    /// Two depositors must never be handed one address.
    ///
    /// A diversifier is only usable when it works for every receiver in the
    /// address, and Sapling rejects about half of them, so `find_address`
    /// walks forward — and consecutive requests collapse onto the same
    /// answer. Asked for 5, 6 and 7 on a real key, all three came back with
    /// one address. Filed under three names, three people's deposits would
    /// have been inseparable.
    #[test]
    fn distinct_indices_never_share_an_address() {
        let network = Network::TestNetwork;
        let mnemonic = <Mnemonic<English>>::generate(Count::Words24);
        let usk =
            UnifiedSpendingKey::from_seed(&network, &mnemonic.to_seed(""), AccountId::ZERO).unwrap();
        let ufvk = usk.to_unified_full_viewing_key();

        let mut seen = std::collections::HashMap::new();
        let mut collapsed = 0;
        for wanted in 0..80u32 {
            let (address, actual) = derive(&ufvk, wanted).unwrap();
            let encoded = address.encode(&network);
            if actual != wanted {
                collapsed += 1;
            }
            if let Some(previous) = seen.insert(encoded.clone(), actual) {
                assert_eq!(
                    previous, actual,
                    "one address came back under two indices, {previous} and {actual}"
                );
            }
        }

        assert!(
            collapsed > 0,
            "no request resolved to a different index; this key would not have caught the bug"
        );
    }
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
        .find(|a| {
            matches!(
                a.as_str(),
                "new" | "scan" | "send" | "address" | "balances" | "withdraw" | "pay" | "serve"
            )
        })
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
            let addresses: u32 = flag("--addresses").unwrap_or_else(|| "64".into()).parse()?;
            scan(&network, &ufvk, birthday, addresses, PathBuf::from(data)).await
        }
        Some("serve") => {
            let ufvk = flag("--ufvk").ok_or_else(|| anyhow!("--ufvk is required"))?;
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            let bind = flag("--bind").unwrap_or_else(|| "127.0.0.1:5321".into());
            serve::run(network, &ufvk, PathBuf::from(data), &bind).await
        }
        Some("withdraw") => {
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            let mut ledger = Ledger::open(&PathBuf::from(&data).join("ledger.sqlite"))?;
            let index: u32 = flag("--index").ok_or_else(|| anyhow!("--index is required"))?.parse()?;
            let zatoshi: u64 = flag("--zatoshi").ok_or_else(|| anyhow!("--zatoshi is required"))?.parse()?;
            let to = flag("--to").ok_or_else(|| anyhow!("--to is required"))?;
            let id = ledger.request_withdrawal(index, zatoshi, &to, now())?;
            println!("withdrawal {id} committed: {zatoshi} zatoshi from address {index}");
            println!("  balance now {} zatoshi", ledger.available(index)?);
            Ok(())
        }
        Some("pay") => {
            let seed = flag("--seed-file").ok_or_else(|| anyhow!("--seed-file is required"))?;
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            pay(&network, PathBuf::from(seed), PathBuf::from(data)).await
        }
        Some("balances") => {
            let data = flag("--data").unwrap_or_else(|| "ring-data".into());
            let ledger = Ledger::open(&PathBuf::from(data).join("ledger.sqlite"))?;
            for (index, label, zatoshi, count) in ledger.balances()? {
                println!(
                    "  address {index:<4} {:>14} zatoshi  {count} deposit(s)  {}",
                    zatoshi,
                    label.unwrap_or_else(|| "(unassigned)".into())
                );
            }
            println!("  total {} zatoshi", ledger.total()?);
            Ok(())
        }
        Some("address") => {
            let ufvk_text = flag("--ufvk").ok_or_else(|| anyhow!("--ufvk is required"))?;
            let ufvk = UnifiedFullViewingKey::decode(&network, &ufvk_text)
                .map_err(|e| anyhow!("that is not a viewing key for this network: {e}"))?;
            let index: u32 = flag("--index").unwrap_or_else(|| "0".into()).parse()?;
            let (address, at) = derive(&ufvk, index)?;
            println!("{}", address.encode(&network));
            if at != index {
                println!("(asked for {index}, the first valid diversifier was {at})");
            }
            Ok(())
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
    let txid = spend(network, &seed_file, to_text, zatoshi, memo_text, &data).await?;
    println!("sent {txid}");
    Ok(())
}

/// Pays every withdrawal that has been committed but not yet sent.
///
/// The transaction id is written down before it is broadcast, so a crash in
/// between leaves something to check the chain for rather than a reason to
/// pay somebody twice.
async fn pay(network: &Network, seed_file: PathBuf, data: PathBuf) -> Result<()> {
    let ledger = Ledger::open(&data.join("ledger.sqlite"))?;
    let owed = ledger.unsent_withdrawals()?;
    if owed.is_empty() {
        println!("nothing owed");
        return Ok(());
    }

    for (id, index, zatoshi, destination) in owed {
        println!("withdrawal {id}: {zatoshi} zatoshi for address {index}");
        let txid = spend(network, &seed_file, &destination, zatoshi, "", &data).await?;
        ledger.mark_sent(id, &txid, now())?;
        println!("  paid by {txid}");
    }
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn spend(
    network: &Network,
    seed_file: &PathBuf,
    to_text: &str,
    zatoshi: u64,
    memo_text: &str,
    data: &PathBuf,
) -> Result<String> {
    let phrase = std::fs::read_to_string(seed_file)
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

    // A transaction carries an expiry height chosen from what the wallet
    // believes the tip is. A wallet that has not been scanned lately builds
    // one that is already expired, and the network refuses it after the
    // proving is done — minutes of work for a transaction born dead.
    {
        let mut client = CompactTxStreamerClient::connect(endpoint(network))
            .await
            .context("reaching lightwalletd")?;
        let tip: u32 = client
            .get_latest_block(ChainSpec {})
            .await
            .context("asking for the chain tip")?
            .into_inner()
            .height
            .try_into()
            .unwrap_or(0);
        let known = db
            .chain_height()
            .map_err(|e| anyhow!("{e}"))?
            .map(u32::from)
            .unwrap_or(0);
        if tip.saturating_sub(known) > 10 {
            bail!(
                "this wallet last saw block {known} and the chain is at {tip}; \
                 run scan first, or the transaction expires before it is sent"
            );
        }
    }

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
        let _ = &raw;

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
        return Ok(txid.to_string());
    }

    anyhow::bail!("nothing was built")
}
