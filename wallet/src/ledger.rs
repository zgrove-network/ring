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
        let ledger = Self { db };
        ledger.prepare()?;
        Ok(ledger)
    }

    fn prepare(&self) -> Result<()> {
        self.db.execute_batch(
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
               ON deposit (address_index);

             CREATE TABLE IF NOT EXISTS round (
               id         INTEGER PRIMARY KEY,
               -- The block whose miner decides it. Chosen by the chain, not
               -- by whoever is betting.
               settles_on INTEGER NOT NULL UNIQUE,
               outcome    TEXT,
               settled_at INTEGER
             );

             CREATE TABLE IF NOT EXISTS bet (
               id            INTEGER PRIMARY KEY,
               round_id      INTEGER NOT NULL REFERENCES round(id),
               address_index INTEGER NOT NULL,
               outcome       TEXT    NOT NULL,
               zatoshi       INTEGER NOT NULL CHECK (zatoshi > 0),
               placed_at     INTEGER NOT NULL,
               -- One position per round per depositor, so a balance cannot be
               -- committed twice to the same block.
               UNIQUE (round_id, address_index)
             );

             CREATE TABLE IF NOT EXISTS payout (
               round_id      INTEGER NOT NULL REFERENCES round(id),
               address_index INTEGER NOT NULL,
               zatoshi       INTEGER NOT NULL,
               PRIMARY KEY (round_id, address_index)
             );

             CREATE TABLE IF NOT EXISTS withdrawal (
               id            INTEGER PRIMARY KEY,
               address_index INTEGER NOT NULL,
               zatoshi       INTEGER NOT NULL CHECK (zatoshi > 0),
               destination   TEXT    NOT NULL,
               requested_at  INTEGER NOT NULL,
               -- Filled in when the transaction is built, before it is
               -- broadcast, so a crash in between leaves something to check
               -- the chain for rather than a reason to pay twice.
               txid          TEXT,
               sent_at       INTEGER
             );

             CREATE INDEX IF NOT EXISTS withdrawal_unsent
               ON withdrawal (address_index) WHERE txid IS NULL;

             CREATE TABLE IF NOT EXISTS rake (
               round_id INTEGER PRIMARY KEY REFERENCES round(id),
               zatoshi  INTEGER NOT NULL
             );",
        )
        .context("preparing the ledger")?;
        Ok(())
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

    /// What a depositor can still commit: what came in, less what is
    /// committed to rounds, plus what rounds returned.
    pub fn available(&self, address_index: u32) -> Result<i64> {
        let deposited: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM deposit WHERE address_index = ?1",
            params![address_index],
            |r| r.get(0),
        )?;
        let committed: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM bet WHERE address_index = ?1",
            params![address_index],
            |r| r.get(0),
        )?;
        let returned: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM payout WHERE address_index = ?1",
            params![address_index],
            |r| r.get(0),
        )?;
        // Counted from the moment it is asked for, sent or not. Waiting for
        // the transaction would let the same balance be withdrawn twice.
        let leaving: i64 = self.db.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM withdrawal WHERE address_index = ?1",
            params![address_index],
            |r| r.get(0),
        )?;
        Ok(deposited - committed + returned - leaving)
    }

    pub fn open_round(&self, settles_on: u32) -> Result<i64> {
        self.db.execute(
            "INSERT OR IGNORE INTO round (settles_on) VALUES (?1)",
            params![settles_on],
        )?;
        let id = self.db.query_row(
            "SELECT id FROM round WHERE settles_on = ?1",
            params![settles_on],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Commits a stake to a round, or refuses.
    ///
    /// The balance is read and the row written inside one transaction. Read
    /// first and write after, and two bets arriving together each see the
    /// money the other is about to spend.
    pub fn place(
        &mut self,
        round_id: i64,
        address_index: u32,
        outcome: &str,
        zatoshi: u64,
        at: i64,
    ) -> Result<()> {
        let tx = self.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let settled: Option<String> = tx.query_row(
            "SELECT outcome FROM round WHERE id = ?1",
            params![round_id],
            |r| r.get(0),
        )?;
        if settled.is_some() {
            anyhow::bail!("round {round_id} is already settled");
        }

        let deposited: i64 = tx.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM deposit WHERE address_index = ?1",
            params![address_index], |r| r.get(0))?;
        let committed: i64 = tx.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM bet WHERE address_index = ?1",
            params![address_index], |r| r.get(0))?;
        let returned: i64 = tx.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM payout WHERE address_index = ?1",
            params![address_index], |r| r.get(0))?;
        let leaving: i64 = tx.query_row(
            "SELECT COALESCE(SUM(zatoshi), 0) FROM withdrawal WHERE address_index = ?1",
            params![address_index], |r| r.get(0))?;

        let available = deposited - committed + returned - leaving;
        let wanted = i64::try_from(zatoshi).context("that stake is not a value")?;
        if wanted > available {
            anyhow::bail!("address {address_index} has {available} zatoshi, not {wanted}");
        }

        tx.execute(
            "INSERT INTO bet (round_id, address_index, outcome, zatoshi, placed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![round_id, address_index, outcome, wanted, at],
        )
        .context("that depositor already has a position in this round")?;
        tx.commit()?;
        Ok(())
    }

    /// Commits a balance to leaving, before anything is sent.
    ///
    /// The debit happens here, not when the transaction goes out. If it
    /// waited, the same balance could be asked for twice and both requests
    /// would look fundable.
    pub fn request_withdrawal(
        &mut self,
        address_index: u32,
        zatoshi: u64,
        destination: &str,
        at: i64,
    ) -> Result<i64> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let sums: [i64; 4] = [
            tx.query_row("SELECT COALESCE(SUM(zatoshi),0) FROM deposit WHERE address_index=?1",
                params![address_index], |r| r.get(0))?,
            tx.query_row("SELECT COALESCE(SUM(zatoshi),0) FROM bet WHERE address_index=?1",
                params![address_index], |r| r.get(0))?,
            tx.query_row("SELECT COALESCE(SUM(zatoshi),0) FROM payout WHERE address_index=?1",
                params![address_index], |r| r.get(0))?,
            tx.query_row("SELECT COALESCE(SUM(zatoshi),0) FROM withdrawal WHERE address_index=?1",
                params![address_index], |r| r.get(0))?,
        ];
        let available = sums[0] - sums[1] + sums[2] - sums[3];
        let wanted = i64::try_from(zatoshi).context("that amount is not a value")?;
        if wanted > available {
            anyhow::bail!("address {address_index} has {available} zatoshi, not {wanted}");
        }

        tx.execute(
            "INSERT INTO withdrawal (address_index, zatoshi, destination, requested_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![address_index, wanted, destination, at],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    /// What has been committed to leaving but has no transaction yet.
    pub fn unsent_withdrawals(&self) -> Result<Vec<(i64, u32, u64, String)>> {
        let mut q = self.db.prepare(
            "SELECT id, address_index, zatoshi, destination
               FROM withdrawal WHERE txid IS NULL ORDER BY id",
        )?;
        let rows = q
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64, r.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Writes down which transaction is paying a withdrawal.
    ///
    /// Refused if one is already recorded: a second transaction for the same
    /// debit is money leaving twice, and nothing on a shielded chain brings
    /// it back.
    pub fn mark_sent(&self, id: i64, txid: &str, at: i64) -> Result<()> {
        let changed = self.db.execute(
            "UPDATE withdrawal SET txid = ?1, sent_at = ?2 WHERE id = ?3 AND txid IS NULL",
            params![txid, at, id],
        )?;
        if changed != 1 {
            let existing: Option<String> = self
                .db
                .query_row("SELECT txid FROM withdrawal WHERE id = ?1", params![id], |r| r.get(0))
                .unwrap_or(None);
            match existing {
                Some(previous) => anyhow::bail!("withdrawal {id} is already paid by {previous}"),
                None => anyhow::bail!("there is no withdrawal {id}"),
            }
        }
        Ok(())
    }
}

/// The house's cut of what the losing side staked, in hundredths.
pub const RAKE_PERCENT: i64 = 3;

#[derive(Debug)]
pub struct Settlement {
    pub outcome: String,
    pub staked: i64,
    pub winners: usize,
    pub paid: i64,
    pub rake: i64,
    pub refunded: bool,
}

impl Ledger {
    /// Closes a round against the miner the chain actually produced.
    ///
    /// Pari-mutuel: those who named the right miner divide what the others
    /// staked. All of it is integer zatoshi — a payout computed in floating
    /// point and rounded at the end is a payout that does not add up, and
    /// this one has to add up exactly, because every zatoshi paid out is a
    /// zatoshi somebody else put in.
    pub fn settle(&mut self, round_id: i64, outcome: &str, at: i64) -> Result<Settlement> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let already: Option<String> = tx.query_row(
            "SELECT outcome FROM round WHERE id = ?1",
            params![round_id],
            |r| r.get(0),
        )?;
        if let Some(previous) = already {
            anyhow::bail!("round {round_id} already settled on {previous}");
        }

        let bets: Vec<(u32, String, i64)> = {
            let mut q = tx.prepare(
                "SELECT address_index, outcome, zatoshi FROM bet WHERE round_id = ?1",
            )?;
            let rows = q
                .query_map(params![round_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        let staked: i64 = bets.iter().map(|(_, _, z)| *z).sum();
        let winners: Vec<&(u32, String, i64)> =
            bets.iter().filter(|(_, o, _)| o == outcome).collect();
        let winner_pool: i64 = winners.iter().map(|(_, _, z)| *z).sum();

        let mut paid = 0i64;
        let mut rake = 0i64;
        let refunded = winner_pool == 0;

        if refunded {
            // Nobody named the miner that turned up. Taking the money anyway
            // would be the house winning on an outcome no one chose, which is
            // the one shape of this that has no defence. Stakes go back.
            for (index, _, stake) in &bets {
                tx.execute(
                    "INSERT INTO payout (round_id, address_index, zatoshi) VALUES (?1, ?2, ?3)",
                    params![round_id, index, stake],
                )?;
                paid += stake;
            }
        } else {
            let losing_pool = staked - winner_pool;
            rake = losing_pool * RAKE_PERCENT / 100;
            let distributable = losing_pool - rake;

            for (index, _, stake) in &winners {
                // Their stake back, plus their share of what the others lost.
                // Widened first: a stake and a pool are each up to the money
                // supply in zatoshi, and their product does not fit in i64.
                let share =
                    i64::try_from(i128::from(distributable) * i128::from(*stake) / i128::from(winner_pool))
                        .context("a share that large cannot be paid")?;
                let amount = stake + share;
                tx.execute(
                    "INSERT INTO payout (round_id, address_index, zatoshi) VALUES (?1, ?2, ?3)",
                    params![round_id, index, amount],
                )?;
                paid += amount;
            }
            // Integer division leaves at most one zatoshi per winner
            // unassigned. It stays with the house rather than vanishing,
            // because the row below has to make the round add up.
            rake = staked - paid;
        }

        tx.execute(
            "INSERT INTO rake (round_id, zatoshi) VALUES (?1, ?2)",
            params![round_id, rake],
        )?;
        tx.execute(
            "UPDATE round SET outcome = ?1, settled_at = ?2 WHERE id = ?3",
            params![outcome, at, round_id],
        )?;

        // Nothing created, nothing lost. If this ever fails the round is
        // rolled back rather than written down wrong.
        if paid + rake != staked {
            anyhow::bail!(
                "round {round_id} does not add up: staked {staked}, paid {paid}, rake {rake}"
            );
        }
        tx.commit()?;

        Ok(Settlement {
            outcome: outcome.to_string(),
            staked,
            winners: winners.len(),
            paid,
            rake,
            refunded,
        })
    }
}

impl Ledger {
    /// The highest block this book has recorded a deposit from.
    ///
    /// Compared against how far the wallet has scanned, this is what says
    /// whether the book is complete or merely not empty.
    pub fn recorded_to(&self) -> Result<Option<u32>> {
        let height: Option<i64> =
            self.db.query_row("SELECT MAX(height) FROM deposit", [], |r| r.get(0))?;
        Ok(height.map(|h| h as u32))
    }

    pub fn is_empty(&self) -> Result<bool> {
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM deposit", [], |r| r.get(0))?;
        Ok(count == 0)
    }

    pub fn total(&self) -> Result<u64> {
        let total: i64 = self
            .db
            .query_row("SELECT COALESCE(SUM(zatoshi), 0) FROM deposit", [], |r| r.get(0))?;
        Ok(total as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Ledger {
        // A ledger in memory, with the same schema the real one gets.
        let db = Connection::open_in_memory().unwrap();
        let ledger = Ledger { db };
        ledger.prepare().unwrap();
        ledger
    }

    fn fund(ledger: &Ledger, index: u32, zatoshi: u64) {
        ledger
            .record_deposit(&Deposit {
                txid: format!("tx{index}"),
                output: 0,
                address_index: index,
                zatoshi,
                height: 1,
                memo: String::new(),
            })
            .unwrap();
    }

    /// A tiny deterministic generator, so a failure can be reproduced from
    /// the seed it printed rather than from "it happened once".
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn a_round_neither_creates_nor_loses_money() {
        // The property that matters most: every zatoshi paid out is a
        // zatoshi somebody staked. Integer division makes this easy to get
        // wrong by a few units per winner, and a few units per round is a
        // hole that widens forever.
        let outcomes = ["unsigned", "Foundry", "Luxor", "2Miners"];

        for seed in 1..200u64 {
            let mut rng = seed;
            let mut ledger = book();
            let players = 2 + (next(&mut rng) % 7) as u32;

            for index in 0..players {
                fund(&ledger, index, 1_000_000);
            }

            let round = ledger.open_round(1000 + seed as u32).unwrap();
            let mut staked_here = 0i64;
            for index in 0..players {
                let stake = 1 + next(&mut rng) % 999_999;
                let pick = outcomes[(next(&mut rng) % outcomes.len() as u64) as usize];
                ledger.place(round, index, pick, stake, 0).unwrap();
                staked_here += stake as i64;
            }

            let winner = outcomes[(next(&mut rng) % outcomes.len() as u64) as usize];
            let settled = ledger.settle(round, winner, 0).unwrap();

            assert_eq!(
                settled.paid + settled.rake,
                staked_here,
                "seed {seed}: staked {staked_here}, paid {} and raked {}",
                settled.paid,
                settled.rake
            );
            assert_eq!(settled.staked, staked_here, "seed {seed}");

            // And nobody is left owing the house.
            for index in 0..players {
                assert!(
                    ledger.available(index).unwrap() >= 0,
                    "seed {seed}: address {index} went negative"
                );
            }
        }
    }

    #[test]
    fn a_winner_is_paid_what_the_rules_say_and_not_less() {
        // Conservation alone cannot see this: a house that quietly keeps a
        // bigger cut still balances the books. So the numbers are pinned.
        //
        //   A stakes 700 on Foundry, B stakes 300 on Luxor, Foundry wins.
        //   losing pool 300, rake 3% of it = 9, 291 to divide,
        //   A is the only winner so takes all of it: 700 + 291 = 991.
        let mut ledger = book();
        fund(&ledger, 0, 1000);
        fund(&ledger, 1, 1000);
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", 700, 0).unwrap();
        ledger.place(round, 1, "Luxor", 300, 0).unwrap();

        let settled = ledger.settle(round, "Foundry", 0).unwrap();
        assert_eq!(settled.rake, 9, "the cut is 3% of what the losers staked");
        assert_eq!(settled.paid, 991);
        assert_eq!(ledger.available(0).unwrap(), 1000 - 700 + 991);
        assert_eq!(ledger.available(1).unwrap(), 700);
    }

    #[test]
    fn two_winners_divide_it_by_what_they_staked() {
        //   A 700 and B 200 on Foundry, C 100 on Luxor, Foundry wins.
        //   losing pool 100, rake 3, 97 to divide by stake:
        //   A gets 97*700/900 = 75, B gets 97*200/900 = 21, and the two
        //   zatoshi integer division leaves over stay with the house.
        let mut ledger = book();
        for index in 0..3 {
            fund(&ledger, index, 1000);
        }
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", 700, 0).unwrap();
        ledger.place(round, 1, "Foundry", 200, 0).unwrap();
        ledger.place(round, 2, "Luxor", 100, 0).unwrap();

        let settled = ledger.settle(round, "Foundry", 0).unwrap();
        assert_eq!(ledger.available(0).unwrap(), 1000 - 700 + 775);
        assert_eq!(ledger.available(1).unwrap(), 1000 - 200 + 221);
        assert_eq!(settled.rake, 4, "3 plus the rounding dust");
    }

    #[test]
    fn the_house_never_takes_more_than_its_cut() {
        // A bound rather than an exact figure, checked over every random
        // round: the cut is three percent of what the losers staked, plus at
        // most one zatoshi per winner from integer division.
        //
        // Half the rounds are run at the scale of the money supply. That is
        // not thoroughness for its own sake: conservation is guaranteed by
        // construction here, because the house takes whatever is left over,
        // so an arithmetic slip does not lose money — it quietly moves it to
        // the house. Only a tight bound on the cut can see that, and only at
        // a scale where the arithmetic can actually slip.
        let outcomes = ["unsigned", "Foundry", "Luxor"];
        for seed in 1..200u64 {
            let mut rng = seed.wrapping_mul(7919);
            let mut ledger = book();
            let players = 2 + (next(&mut rng) % 6) as u32;
            let purse = if seed % 2 == 0 { 400_000_000_000_000u64 } else { 1_000_000u64 };
            for index in 0..players {
                fund(&ledger, index, purse);
            }
            let round = ledger.open_round(2000 + seed as u32).unwrap();
            let mut pools = std::collections::HashMap::new();
            let mut staked_by: Vec<(u32, &str, i64)> = Vec::new();
            for index in 0..players {
                let stake = 1 + next(&mut rng) % purse;
                let pick = outcomes[(next(&mut rng) % outcomes.len() as u64) as usize];
                ledger.place(round, index, pick, stake, 0).unwrap();
                *pools.entry(pick).or_insert(0i64) += stake as i64;
                staked_by.push((index, pick, stake as i64));
            }
            let winner = outcomes[(next(&mut rng) % outcomes.len() as u64) as usize];
            let settled = ledger.settle(round, winner, 0).unwrap();
            if settled.refunded {
                assert_eq!(settled.rake, 0, "seed {seed}");
                continue;
            }
            let winner_pool = pools.get(winner).copied().unwrap_or(0);
            let losing = settled.staked - winner_pool;
            let ceiling = losing * RAKE_PERCENT / 100 + settled.winners as i64;
            assert!(
                settled.rake <= ceiling,
                "seed {seed}: took {} of a losing pool of {losing}, ceiling {ceiling}",
                settled.rake
            );

            // And each winner to the zatoshi, worked out here independently.
            // A bound alone tolerates a payout that is merely close, and
            // "close" repeated a million times is a number somebody notices.
            let rake = losing * RAKE_PERCENT / 100;
            let distributable = losing - rake;
            for (index, pick, stake) in &staked_by {
                if *pick != winner {
                    continue;
                }
                let share = i64::try_from(
                    i128::from(distributable) * i128::from(*stake) / i128::from(winner_pool),
                )
                .unwrap();
                let expected = purse as i64 - stake + stake + share;
                assert_eq!(
                    ledger.available(*index).unwrap(),
                    expected,
                    "seed {seed}: address {index} staked {stake} of {winner_pool}"
                );
            }
        }
    }

    #[test]
    fn stakes_near_the_money_supply_do_not_overflow() {
        // Zcash's supply is 2.1e15 zatoshi. Multiplying a stake by a pool at
        // that scale leaves i64 far behind, and the product is exactly what
        // dividing a pool by stake requires.
        let huge = 2_000_000_000_000_000u64;
        let mut ledger = book();
        fund(&ledger, 0, huge);
        fund(&ledger, 1, huge);
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", huge, 0).unwrap();
        ledger.place(round, 1, "Luxor", huge, 0).unwrap();

        let settled = ledger.settle(round, "Foundry", 0).unwrap();
        assert_eq!(settled.paid + settled.rake, settled.staked);
        assert!(settled.paid > huge as i64, "the winner is ahead");
    }

    #[test]
    fn nobody_can_stake_what_they_do_not_have() {
        let mut ledger = book();
        fund(&ledger, 0, 500);
        let round = ledger.open_round(10).unwrap();

        assert!(ledger.place(round, 0, "Foundry", 501, 0).is_err(), "overdrawn");
        ledger.place(round, 0, "Foundry", 500, 0).unwrap();

        let second = ledger.open_round(11).unwrap();
        assert!(
            ledger.place(second, 0, "Luxor", 1, 0).is_err(),
            "the first bet already committed it"
        );
    }

    #[test]
    fn one_position_per_round() {
        let mut ledger = book();
        fund(&ledger, 0, 1000);
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", 100, 0).unwrap();
        assert!(ledger.place(round, 0, "Luxor", 100, 0).is_err());
    }

    #[test]
    fn a_round_nobody_called_right_is_refunded_rather_than_kept() {
        // The house winning on an outcome nobody chose is the one shape of
        // this with no defence.
        let mut ledger = book();
        fund(&ledger, 0, 1000);
        fund(&ledger, 1, 1000);
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", 700, 0).unwrap();
        ledger.place(round, 1, "Luxor", 300, 0).unwrap();

        let settled = ledger.settle(round, "2Miners", 0).unwrap();
        assert!(settled.refunded);
        assert_eq!(settled.rake, 0, "the house takes nothing");
        assert_eq!(ledger.available(0).unwrap(), 1000);
        assert_eq!(ledger.available(1).unwrap(), 1000);
    }

    #[test]
    fn a_withdrawal_is_debited_when_it_is_asked_for() {
        // Not when it is sent. Waiting would let the same balance be asked
        // for twice, and both requests would look fundable.
        let mut ledger = book();
        fund(&ledger, 0, 1000);

        ledger.request_withdrawal(0, 600, "utest1somewhere", 0).unwrap();
        assert_eq!(ledger.available(0).unwrap(), 400, "debited before sending");
        assert!(
            ledger.request_withdrawal(0, 500, "utest1somewhere", 0).is_err(),
            "only 400 is left"
        );
        ledger.request_withdrawal(0, 400, "utest1somewhere", 0).unwrap();
        assert_eq!(ledger.available(0).unwrap(), 0);
    }

    #[test]
    fn a_withdrawal_is_not_paid_by_two_transactions() {
        // Nothing on a shielded chain brings the second one back.
        let mut ledger = book();
        fund(&ledger, 0, 1000);
        let id = ledger.request_withdrawal(0, 500, "utest1somewhere", 0).unwrap();

        ledger.mark_sent(id, "aa".repeat(32).as_str(), 0).unwrap();
        let second = ledger.mark_sent(id, "bb".repeat(32).as_str(), 0);
        assert!(second.is_err(), "paid twice");
        assert!(format!("{}", second.unwrap_err()).contains("already paid"));

        assert!(ledger.unsent_withdrawals().unwrap().is_empty());
    }

    #[test]
    fn money_committed_to_a_round_cannot_also_be_withdrawn() {
        let mut ledger = book();
        fund(&ledger, 0, 1000);
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", 800, 0).unwrap();

        assert!(ledger.request_withdrawal(0, 300, "utest1somewhere", 0).is_err());
        ledger.request_withdrawal(0, 200, "utest1somewhere", 0).unwrap();

        // And what the round returns is withdrawable afterwards.
        ledger.settle(round, "Foundry", 0).unwrap();
        assert_eq!(ledger.available(0).unwrap(), 800, "the stake came back");
    }

    #[test]
    fn a_settled_round_is_not_settled_twice() {
        let mut ledger = book();
        fund(&ledger, 0, 1000);
        let round = ledger.open_round(10).unwrap();
        ledger.place(round, 0, "Foundry", 100, 0).unwrap();
        ledger.settle(round, "Foundry", 0).unwrap();
        assert!(ledger.settle(round, "Luxor", 0).is_err());
        assert!(ledger.place(round, 0, "Luxor", 100, 0).is_err());
    }
}
