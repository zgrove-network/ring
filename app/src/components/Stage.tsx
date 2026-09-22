import { UNSIGNED, type Block } from "../lib/chain";
import { clock } from "../lib/market";
import { Rings } from "./Rings";

interface Props {
  readonly blocks: readonly Block[];
  readonly elapsed: number;
  readonly poolTotal: number;
  readonly picked: string | null;
}

export function Stage({ blocks, elapsed, poolTotal, picked }: Props) {
  const last = blocks[0];

  return (
    <section className="stage">
      <Rings blocks={blocks} picked={picked} />

      <div className="stage-grid">
        <div className="cell hero">
          <span className="k">since last block</span>
          <span className="figure">{clock(elapsed)}</span>
        </div>

        <div className="cell">
          <span className="k">last taken by</span>
          <span className="v big">
            {last === undefined ? "—" : last.miner === UNSIGNED ? "unsigned" : last.miner}
          </span>
        </div>

        <div className="cell">
          <span className="k">staked</span>
          <span className="v big">{poolTotal.toFixed(1)}</span>
        </div>

        {/* The size of the pool is public; how it is spread across the miners
            is not. That spread is the only thing held back, and it is exactly
            what an open book would give away. */}
        <div className="cell">
          <span className="k">spread</span>
          <span className="v big sealed">
            ???
            <span className="caret" aria-hidden="true">
              _
            </span>
          </span>
        </div>

        <div className="cell clock-cell">
          <span className="k">your pick</span>
          <span className={picked === null ? "clock faint" : "clock under"}>
            {picked === null ? "none" : picked === UNSIGNED ? "unsigned" : picked}
          </span>
        </div>
      </div>
    </section>
  );
}
