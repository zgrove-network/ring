import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { recentBlocks, type Block } from "../lib/chain";
import {
  outcomeOf,
  settle,
  sharesOver,
  type Position,
  type Round,
} from "../lib/market";

/** How often the explorer is asked whether a block has landed. Zcash aims for
 * one every 75 seconds, so this is frequent enough to feel live without
 * hammering a service we do not own. */
const POLL_MS = 12_000;

export type Source = "chain" | "offline";

export interface MarketState {
  readonly source: Source;
  readonly blocks: readonly Block[];
  readonly rounds: readonly Round[];
  readonly position: Position | null;
  readonly balance: number;
  readonly session: number;
  readonly hits: number;
  readonly staked: number;
  /** Seconds since the last block landed. */
  readonly elapsed: number;
}

/** The other side of the market is not real. Blocks are; counterparties are
 * simulated, and the interface says so rather than implying a crowd that is
 * not there. Money spreads roughly along recent share, with enough noise that
 * the odds move between rounds. */
function drawPools(outcomes: readonly string[]): ReadonlyMap<string, number> {
  const pools = new Map<string, number>();
  for (const name of outcomes) {
    pools.set(name, 0.4 + Math.random() * 3.2);
  }
  return pools;
}

export function useMarket() {
  const [blocks, setBlocks] = useState<readonly Block[]>([]);
  const [rounds, setRounds] = useState<readonly Round[]>([]);
  const [position, setPosition] = useState<Position | null>(null);
  const [pools, setPools] = useState<ReadonlyMap<string, number>>(new Map());
  const [balance, setBalance] = useState(1);
  const [session, setSession] = useState(0);
  const [hits, setHits] = useState(0);
  const [staked, setStaked] = useState(0);
  const [elapsed, setElapsed] = useState(0);
  const [source, setSource] = useState<Source>("offline");

  const positionRef = useRef<Position | null>(null);
  positionRef.current = position;
  const poolsRef = useRef<ReadonlyMap<string, number>>(pools);
  poolsRef.current = pools;
  const tipRef = useRef<number>(0);

  const absorb = useCallback((fresh: readonly Block[]) => {
    const newest = fresh[0];
    if (newest === undefined) return;

    setBlocks(fresh);
    const listed = sharesOver(fresh);
    const names = listed.map((s) => s.miner);

    // First load: everything fetched is history. It already happened and
    // nobody held a position on it, but showing it beats an empty table.
    if (tipRef.current === 0) {
      const seeded = fresh
        .slice(0, 40)
        .map((b) => settle(b, outcomeOf(b, listed), drawPools(names), null));
      setRounds(seeded);
      const opening = drawPools(names);
      setPools(opening);
      poolsRef.current = opening;
      tipRef.current = newest.height;
      return;
    }

    const landed = fresh
      .filter((b) => b.height > tipRef.current)
      .sort((a, b) => a.height - b.height);

    for (const block of landed) {
      const held = positionRef.current;

      // A memo in flight is carried by this block, which fixes the round it
      // bets on: the next one, which does not exist yet.
      if (held !== null && held.stage === "sent") {
        const confirmed: Position = {
          ...held,
          stage: "confirmed",
          confirmedIn: block.height,
          target: block.height + 1,
        };
        setPosition(confirmed);
        positionRef.current = confirmed;
      }

      const mine = held !== null && held.stage === "confirmed" && held.target === block.height
        ? held
        : null;
      const round = settle(block, outcomeOf(block, listed), poolsRef.current, mine);

      setRounds((prev) => [round, ...prev].slice(0, 40));
      if (mine !== null) {
        setBalance((b) => b + round.delta);
        setSession((s) => s + round.delta);
        setStaked((n) => n + 1);
        if (round.delta > 0) setHits((n) => n + 1);
        setPosition(null);
        positionRef.current = null;
      }
      const next = drawPools(names);
      setPools(next);
      poolsRef.current = next;
    }

    tipRef.current = newest.height;
  }, []);

  useEffect(() => {
    let alive = true;

    async function pull() {
      try {
        const fresh = await recentBlocks();
        if (!alive) return;
        absorb(fresh);
        setSource("chain");
      } catch {
        if (!alive) return;
        setSource("offline");
      }
    }

    void pull();
    const id = window.setInterval(() => void pull(), POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [absorb]);

  useEffect(() => {
    const tick = () => {
      const newest = blocks[0];
      setElapsed(newest === undefined ? 0 : Math.max(0, (Date.now() - newest.at) / 1000));
    };
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [blocks]);

  const shares = useMemo(() => sharesOver(blocks), [blocks]);

  /** Broadcasts the memo. Which round it joins is not ours to say. */
  const take = useCallback((miner: string, stake: number) => {
    setPosition({ miner, stake, stage: "sent", confirmedIn: null, target: null });
  }, []);

  const state: MarketState = {
    source,
    blocks,
    rounds,
    position,
    balance,
    session,
    hits,
    staked,
    elapsed,
  };

  return { state, shares, pools, take };
}
