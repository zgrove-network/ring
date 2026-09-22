import { useState } from "react";

import { UNSIGNED } from "../lib/chain";
import {
  backtest,
  hitRate,
  signed,
  type Position,
  type Round,
  type Share,
} from "../lib/market";

interface Props {
  readonly position: Position | null;
  readonly shares: readonly Share[];
  readonly rounds: readonly Round[];
  readonly picked: string | null;
  readonly onPick: (miner: string) => void;
  readonly onTake: (miner: string, stake: number) => void;
}

/** The unsigned bucket needs a line of its own, because "nobody signed it" is
 * a fact about the coinbase and not a claim about who mined it. */
function label(miner: string): string {
  return miner === UNSIGNED ? "unsigned" : miner;
}

export function SidePanel({ position, shares, rounds, picked, onPick, onTake }: Props) {
  const [draft, setDraft] = useState("");

  const typed = Number.parseFloat(draft);
  const valid = Number.isFinite(typed) && typed > 0 && picked !== null;

  if (position !== null) {
    const flying = position.stage === "sent";
    return (
      <section className="panel">
        <h2>position</h2>
        <div className="box sealed-box">
          <span className="figure mid">{label(position.miner)}</span>
        </div>

        {/* Three states, and the middle one is the point: the chain, not the
            bettor, decided which round this joined. */}
        <ol className="steps">
          <li className="done">
            <span className="step-k">broadcast</span>
            <span className="step-v">memo sent</span>
          </li>
          <li className={flying ? "now" : "done"}>
            <span className="step-k">carried by</span>
            <span className="step-v">
              {flying
                ? "waiting for a block"
                : `block ${position.confirmedIn?.toLocaleString("en-US") ?? "—"}`}
            </span>
          </li>
          <li className={flying ? undefined : "now"}>
            <span className="step-k">settles on</span>
            <span className="step-v">
              {flying
                ? "the block after that"
                : `block ${position.target?.toLocaleString("en-US") ?? "—"}`}
            </span>
          </li>
        </ol>

        <dl className="rows">
          <dt>stake</dt>
          <dd className="bright">{signed(position.stake).slice(1)}</dd>
          <dt>state</dt>
          <dd className="live">sealed</dd>
        </dl>
      </section>
    );
  }

  const odds = picked === null ? null : hitRate(picked, rounds);
  const projected = picked === null ? null : backtest(picked, rounds);

  return (
    <section className="panel">
      <h2>who takes the next block</h2>

      <div className="miners">
        {shares.map((s) => (
          <button
            key={s.miner}
            type="button"
            className={picked === s.miner ? "miner picked" : "miner"}
            onClick={() => onPick(s.miner)}
          >
            <span className="name">{label(s.miner)}</span>
            <span className="share">{(s.share * 100).toFixed(0)}%</span>
          </button>
        ))}
      </div>

      <div className="box">
        <input
          id="stake"
          type="text"
          inputMode="decimal"
          autoComplete="off"
          placeholder="0.0000"
          aria-label="stake, in ZEC"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && valid && picked !== null) onTake(picked, typed);
          }}
        />
        <span className="unit">ZEC</span>
      </div>

      <dl className="rows">
        <dt>took, last {odds?.of ?? 20}</dt>
        <dd className={odds === null ? "faint" : undefined}>
          {odds === null ? "—" : `${odds.hits}/${odds.of}`}
        </dd>
        <dt>backing it every block</dt>
        <dd className={projected === null ? "faint" : projected >= 0 ? "good" : "bad"}>
          {projected === null ? "—" : signed(projected)}
        </dd>
        <dt>what it pays</dt>
        <dd className="faint">sealed</dd>
      </dl>

      <button
        type="button"
        className="primary"
        disabled={!valid}
        onClick={() => picked !== null && onTake(picked, typed)}
      >
        seal
      </button>
    </section>
  );
}
