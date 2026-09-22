import { UNSIGNED } from "../lib/chain";
import { WINDOW, type Share } from "../lib/market";

interface Props {
  readonly shares: readonly Share[];
  readonly counted: number;
  readonly picked: string | null;
}

/** Share of the last blocks, as one bar per outcome. The bar a viewer has
 * money on is lit; the rest sit back. */
export function StatusBar({ shares, counted, picked }: Props) {
  return (
    <footer className="statusbar">
      <div className="chart-cell">
        <span className="k">
          who took the last {Math.min(counted, WINDOW)} blocks
        </span>
        <div className="shares">
          {shares.map((s) => (
            <div
              key={s.miner}
              className={picked === s.miner ? "share-row lit" : "share-row"}
              style={{ flexGrow: Math.max(s.share, 0.02) }}
              title={`${s.miner} ${(s.share * 100).toFixed(1)}%`}
            >
              <span className="share-bar" />
              <span className="share-name">
                {s.miner === UNSIGNED ? "unsigned" : s.miner}
              </span>
            </div>
          ))}
        </div>
      </div>

      <dl className="figures">
        {shares.slice(0, 4).map((s) => (
          <div key={s.miner}>
            <dt>{s.miner === UNSIGNED ? "unsigned" : s.miner}</dt>
            <dd>{(s.share * 100).toFixed(0)}%</dd>
          </div>
        ))}
      </dl>
    </footer>
  );
}
