/** A pari-mutuel market on which miner takes the next Zcash block.
 *
 * Everyone who backed the winner splits the rest of the pool in proportion to
 * what they put in, less the house's cut. Nobody trades against the house,
 * so the house cannot want an outcome — and could not reach it if it did,
 * because reaching it means buying hashrate.
 *
 * Stakes ride in shielded memos, so until a block lands nobody can see how
 * the pool leans. That matters here in a way it would not on a coin flip:
 * published pool shares are computed over 24-hour windows, so a miner moving
 * between pools is visible in the block stream hours before any dashboard
 * catches up. Somebody watching the chain knows something. Sealing the book
 * is what lets them act on it without giving it away. */

import { UNSIGNED, type Block } from "./chain";

/** The house's cut of the losing pool. */
export const RAKE = 0.03;

/** How many blocks the displayed shares look back over. */
export const WINDOW = 100;

/** Outcomes shown as their own button. Everything rarer is bucketed, because
 * a market with thirty outcomes has thirty empty pools. */
export const SHOWN = 5;

/** The bucket for miners too rare to list. */
export const FIELD = "the field";

export interface Share {
  readonly miner: string;
  readonly blocks: number;
  readonly share: number;
}

/** A position does not choose its round. The memo is broadcast, the chain
 * decides which block carries it, and it settles on the block after that one.
 *
 * This is what makes the timing honest without a betting window: the block
 * your bet settles on does not exist yet when your bet is written down, so
 * nobody — you, the house, or a miner — can have seen the result. It also
 * kills last-second sniping outright, because nobody picks which block their
 * memo lands in. */
export type Stage = "sent" | "confirmed";

export interface Position {
  readonly miner: string;
  readonly stake: number;
  readonly stage: Stage;
  /** Block the memo landed in. Null while it is still in flight. */
  readonly confirmedIn: number | null;
  /** Block this settles on: one after the memo's. Null until confirmed. */
  readonly target: number | null;
}

export interface Round {
  readonly block: Block;
  readonly miner: string;
  readonly pools: ReadonlyMap<string, number>;
  readonly payout: number;
  readonly position: Position | null;
  readonly delta: number;
}

/** Recent share by miner, commonest first, with everything past the listed
 * few folded into one bucket. */
export function sharesOver(blocks: readonly Block[]): readonly Share[] {
  const seen = blocks.slice(0, WINDOW);
  if (seen.length === 0) return [];

  const counts = new Map<string, number>();
  for (const b of seen) counts.set(b.miner, (counts.get(b.miner) ?? 0) + 1);

  const ranked = [...counts.entries()]
    .map(([miner, count]) => ({ miner, count }))
    .sort((a, b) => b.count - a.count);

  // The unsigned bucket is an outcome in its own right, never folded away.
  const listed = ranked.filter((r) => r.miner === UNSIGNED).concat(
    ranked.filter((r) => r.miner !== UNSIGNED).slice(0, SHOWN),
  );
  const rest = ranked
    .filter((r) => !listed.includes(r))
    .reduce((sum, r) => sum + r.count, 0);

  const out: Share[] = listed.map((r) => ({
    miner: r.miner,
    blocks: r.count,
    share: r.count / seen.length,
  }));

  if (rest > 0) {
    out.push({ miner: FIELD, blocks: rest, share: rest / seen.length });
  }
  return out;
}

/** Which listed outcome a block settles to. */
export function outcomeOf(block: Block, listed: readonly Share[]): string {
  return listed.some((s) => s.miner === block.miner) ? block.miner : FIELD;
}

export function payoutFor(pools: ReadonlyMap<string, number>, outcome: string): number {
  const mine = pools.get(outcome) ?? 0;
  if (mine <= 0) return 0;
  let total = 0;
  for (const v of pools.values()) total += v;
  return 1 + ((total - mine) / mine) * (1 - RAKE);
}

export function settle(
  block: Block,
  outcome: string,
  pools: ReadonlyMap<string, number>,
  position: Position | null,
): Round {
  const payout = payoutFor(pools, outcome);

  let delta = 0;
  if (position !== null) {
    delta = position.miner === outcome ? position.stake * (payout - 1) : -position.stake;
  }

  return { block, miner: outcome, pools, payout, position, delta };
}

/** What backing one miner every block would have returned lately. */
export function backtest(miner: string, rounds: readonly Round[], over = 20): number {
  let total = 0;
  for (const r of rounds.slice(0, over)) {
    total += r.miner === miner ? payoutFor(r.pools, miner) - 1 : -1;
  }
  return total;
}

export function hitRate(
  miner: string,
  rounds: readonly Round[],
  over = 20,
): { hits: number; of: number } {
  const seen = rounds.slice(0, over);
  return { hits: seen.filter((r) => r.miner === miner).length, of: seen.length };
}

export function zec(value: number): string {
  return value.toFixed(4);
}

export function signed(value: number): string {
  return (value >= 0 ? "+" : "") + value.toFixed(4);
}

export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}
