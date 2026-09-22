import { Mark } from "./Mark";
import type { Source } from "../hooks/useMarket";

interface Props {
  readonly height: number | null;
  readonly source: Source;
}

export function TopBar({ height, source }: Props) {
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
        {source === "chain" ? "zcash mainnet" : "explorer offline"}
      </span>
      <span className="tb">
        <span className="k">counterparties</span>
        <span className="v">simulated</span>
      </span>
    </header>
  );
}
