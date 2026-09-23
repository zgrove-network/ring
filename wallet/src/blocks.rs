use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

// Who took a block, read from its coinbase.
//
// This is the same reading the market shows, and it has to be: a player who
// watched "Foundry" take a block and then finds the ledger paid "unsigned"
// has been told two different truths about the same money. The market's copy
// is in TypeScript and describes; this one decides.

/// Blocks whose coinbase carries no recognisable name — about half of them,
/// which lines up with the two largest pools by published share not signing
/// theirs. The honest label is that nobody signed it, which anyone can check;
/// naming a suspect would be a guess presented as a fact.
pub const UNSIGNED: &str = "unsigned";

/// Matched case-insensitively against the printable bytes, longest first so a
/// shorter name cannot eat a longer one.
const KNOWN: &[&str] = &[
    "zecminingpool",
    "milledgeville",
    "HeroMiners",
    "flexpool",
    "NiceHash",
    "2Miners",
    "Foundry",
    "Kryptex",
    "zergpool",
    "AntPool",
    "sluicey",
    "KuPool",
    "F2Pool",
    "ViaBTC",
    "poolin",
    "Luxor",
];

#[derive(Debug, Deserialize)]
pub struct Raw {
    pub id: u32,
    pub coinbase_data_hex: Option<String>,
    pub guessed_miner: Option<String>,
}

pub fn miner_of(row: &Raw) -> String {
    if let Some(guess) = row.guessed_miner.as_deref() {
        if !guess.is_empty() && !guess.eq_ignore_ascii_case("unknown") {
            return guess.to_string();
        }
    }

    let Some(hex) = row.coinbase_data_hex.as_deref() else {
        return UNSIGNED.to_string();
    };
    if hex.is_empty() {
        return UNSIGNED.to_string();
    }

    // Printable bytes only, everything else a space, so a name cannot be
    // spelled across binary padding that happens to sit between its letters.
    let digits: Vec<char> = hex.chars().collect();
    let mut text = String::with_capacity(digits.len() / 2);
    for pair in digits.chunks(2) {
        let byte = u8::from_str_radix(&pair.iter().collect::<String>(), 16).ok();
        match byte {
            Some(b) if (32..127).contains(&b) => text.push(b as char),
            _ => text.push(' '),
        }
    }
    let text = text.to_lowercase();

    for name in KNOWN {
        if text.contains(&name.to_lowercase()) {
            return (*name).to_string();
        }
    }
    UNSIGNED.to_string()
}

#[derive(Deserialize)]
struct Answer {
    data: Vec<Raw>,
}

/// One block from the explorer, or nothing if it is not mined yet.
pub fn at_height(source: &str, height: u32) -> Result<Option<Raw>> {
    let url = format!("{source}?q=id({height})&fields=id,coinbase_data_hex,guessed_miner");
    let response = minreq::get(&url)
        .with_timeout(20)
        .send()
        .with_context(|| format!("asking the explorer for block {height}"))?;
    if response.status_code != 200 {
        return Err(anyhow!(
            "the explorer answered {} for block {height}; this says nothing about the block",
            response.status_code
        ));
    }
    let answer: Answer = serde_json::from_str(response.as_str()?)
        .context("the explorer's answer was not a block list")?;

    match answer.data.into_iter().next() {
        None => Ok(None),
        // Asked about one height and told about another. Settling on it
        // would pay out against the wrong block, so it is refused rather
        // than trusted.
        Some(block) if block.id != height => Err(anyhow!(
            "asked the explorer for block {height} and it answered about {}",
            block.id
        )),
        Some(block) => Ok(Some(block)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(hex: &str, guess: Option<&str>) -> Raw {
        Raw {
            id: 1,
            coinbase_data_hex: Some(hex.to_string()),
            guessed_miner: guess.map(str::to_string),
        }
    }

    fn hex_of(text: &str) -> String {
        text.bytes().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_name_in_the_coinbase_is_the_miner() {
        assert_eq!(
            miner_of(&row(&hex_of("mined by 2Miners pool"), None)),
            "2Miners"
        );
        assert_eq!(
            miner_of(&row(&hex_of("\u{1}\u{2}Foundry USA"), None)),
            "Foundry"
        );
        assert_eq!(miner_of(&row(&hex_of("viabtc.com"), None)), "ViaBTC");
    }

    #[test]
    fn a_coinbase_with_no_name_is_unsigned_rather_than_guessed_at() {
        // Naming a suspect would be a guess presented as a fact, and it would
        // decide who gets paid.
        assert_eq!(miner_of(&row(&hex_of("just some bytes"), None)), UNSIGNED);
        assert_eq!(miner_of(&row("", None)), UNSIGNED);
        assert_eq!(
            miner_of(&Raw {
                id: 1,
                coinbase_data_hex: None,
                guessed_miner: None
            }),
            UNSIGNED
        );
    }

    #[test]
    fn the_explorers_own_guess_wins_when_it_has_one() {
        assert_eq!(miner_of(&row(&hex_of("2Miners"), Some("Luxor"))), "Luxor");
        // But "unknown" is not a guess.
        assert_eq!(
            miner_of(&row(&hex_of("2Miners"), Some("unknown"))),
            "2Miners"
        );
        assert_eq!(miner_of(&row(&hex_of("2Miners"), Some(""))), "2Miners");
    }

    #[test]
    fn a_name_is_not_spelled_across_unprintable_padding() {
        // Non-printable bytes become spaces, so "Lu\0xor" is not Luxor.
        let broken = format!("{}{}{}", hex_of("Lu"), "00", hex_of("xor"));
        assert_eq!(miner_of(&row(&broken, None)), UNSIGNED);
    }
}
