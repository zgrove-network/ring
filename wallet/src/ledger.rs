use anyhow::{Context, Result};
use rusqlite::{params, Connection};

/// The book: who deposited what, and what they are owed.
///
/// The chain is the authority for money arriving and leaving. This records
/// what the chain showed and, later, what happened between those two events —
/// the rounds, which never touch the chain and so exist only here.
///
/// That is the trade the account model makes: a round is fast and free and
/// invisible, and in exchange its record is ours rather than the chain's.
/// Which is why every balance here has to be reconstructible from rows that
/// name their evidence.
pub struct Ledger {
    db: Connection,
}

#[derive(Debug, Clone)]
pub struct Deposit {
    pub txid: String,
    pub output: u32,
    pub address_index: u32,
    pub zatoshi: u64,
    pub height: u32,
    pub memo: String,
}

impl Ledger {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let db = Connection::open(path).context("opening the ledger")?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             -- A payout is decided from these rows, so a write that is
             -- acknowledged has to survive the power going out.
             PRAGMA synchronous = FULL;

             CREATE TABLE IF NOT EXISTS depositor (
               address_index INTEGER PRIMARY KEY,
               label         TEXT    NOT NULL,
               created_at    INTEGER NOT NULL
             );

             CREATE TABLE IF NOT EXISTS deposit (
               txid          TEXT    NOT NULL,
               output        INTEGER NOT NULL,
               address_index INTEGER NOT NULL,
               zatoshi       INTEGER NOT NULL,
               height        INTEGER NOT NULL,
               memo          TEXT    NOT NULL,
               -- One note, one row, however many times the chain is read.
               -- Scanning is repeated by design; crediting twice is not.
               PRIMARY KEY (txid, output)
             );

             CREATE INDEX IF NOT EXISTS deposit_by_depositor
               ON deposit (address_index);",
        )
        .context("preparing the ledger")?;
        Ok(Self { db })
    }

    /// Records a deposit, or does nothing if this note was already recorded.
    ///
    /// Returns whether the row was new, so a rescan can say plainly that it
    /// found nothing rather than appearing to have done work.
    pub fn record_deposit(&self, deposit: &Deposit) -> Result<bool> {
        let changed = self
            .db
            .execute(
                "INSERT OR IGNORE INTO deposit
                   (txid, output, address_index, zatoshi, height, memo)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    deposit.txid,
                    deposit.output,
                    deposit.address_index,
                    deposit.zatoshi,
                    deposit.height,
                    deposit.memo
                ],
            )
            .context("recording a deposit")?;
        Ok(changed == 1)
    }

    pub fn assign(&self, address_index: u32, label: &str, at: i64) -> Result<()> {
        self.db
            .execute(
                "INSERT OR IGNORE INTO depositor (address_index, label, created_at)
                 VALUES (?1, ?2, ?3)",
                params![address_index, label, at],
            )
            .context("assigning an address")?;
        Ok(())
    }

    /// What each depositor has put in, deepest first.
    pub fn balances(&self) -> Result<Vec<(u32, Option<String>, u64, u32)>> {
        let mut statement = self.db.prepare(
            "SELECT d.address_index,
                    (SELECT label FROM depositor p WHERE p.address_index = d.address_index),
                    SUM(d.zatoshi),
                    COUNT(*)
               FROM deposit d
              GROUP BY d.address_index
              ORDER BY SUM(d.zatoshi) DESC",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as u64, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn total(&self) -> Result<u64> {
        let total: i64 = self
            .db
            .query_row("SELECT COALESCE(SUM(zatoshi), 0) FROM deposit", [], |r| r.get(0))?;
        Ok(total as u64)
    }
}
