import { Mark } from "./Mark";
import type { Money, Source } from "../hooks/useMarket";

interface Props {
  readonly height: number | null;
  readonly source: Source;
  readonly ageSeconds: number;
  readonly money: Money;
}

export function TopBar({ height, source, ageSeconds, money }: Props) {
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
      {/* Whichever of these is true has to be the one on screen. A page
          showing a balance is read as an account somebody is holding, and
          saying "no real money" while holding real money is the worse of the
          two lies. */}
      {money === "simulated" ? (
        <span
          className="badge sim"
          title="No wallet is connected and no payment is made. The blocks are real; the stakes are not."
        >
          simulation · no real money
        </span>
      ) : money === "ledger" ? (
        <span className="badge live" title="Balances here are the ledger's, in ZEC.">
          real balance
        </span>
      ) : (
        <span className="badge bad" title="There is a ledger, and this page cannot reach it. Nothing shown is a balance.">
          ledger unreachable
        </span>
      )}
    </header>
  );
}
