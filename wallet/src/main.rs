use anyhow::{bail, Context, Result};
use bip0039::{Count, English, Mnemonic};
use zcash_client_backend::keys::UnifiedAddressRequest;
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

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("new") => generate(),
        _ => bail!("usage: ring-wallet new"),
    }
}
