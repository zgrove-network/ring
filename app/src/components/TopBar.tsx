import { Mark } from "./Mark";
import type { Source } from "../hooks/useMarket";

interface Props {
  readonly height: number | null;
  readonly source: Source;
  readonly ageSeconds: number;
}

export function TopBar({ height, source, ageSeconds }: Props) {
  return (
    <header className="topbar">
      <Mark />
      <span className="wordmark">ring</span>

      <span className="tb">
        <span className="k">next block</span>
        <span className="v">{height === null ? "—" : (height + 1).toLocaleString("en-US")}</span>
      </span>

      <span className="grow" />

      <span className={source === "chain" ? "badge live" : "badge"}>
        {source === "chain"
          ? "zcash mainnet"
          : source === "stale"
            ? `mainnet · ${Math.round(ageSeconds / 60) || 1}m old`
            : "explorer offline"}
      </span>
      {/* "simulated counterparties" could be read as "the others are bots but
          my money is real". It is not: nothing here touches a wallet. */}
      <span className="badge sim" title="No wallet is connected and no payment is made. The blocks are real; the stakes are not.">
        simulation · no real money
      </span>
    </header>
  );
}
