import { Mark } from "./Mark";
import type { Money, Source } from "../hooks/useMarket";

interface Props {
  readonly height: number | null;
  readonly source: Source;
  readonly ageSeconds: number;
  readonly money: Money;
  readonly network: string | null;
}

export function TopBar({ height, source, ageSeconds, money, network }: Props) {
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
      {/* A badge is a warning, so there is one only when something needs
          saying. The ordinary case — a real balance on mainnet — says
          nothing, and the chain badge beside it already says which chain. */}
      {money === "simulated" ? (
        <span
          className="badge sim"
          title="No wallet is connected and no payment is made. The blocks are real; the stakes are not."
        >
          simulation · no real money
        </span>
      ) : money === "ledger" && network === "testnet" ? (
        <span className="badge sim" title="Testnet coins. They are worth nothing and cannot be exchanged.">
          testnet · coins worth nothing
        </span>
      ) : money === "ledger" ? (
        /* Nothing. A badge means there is something to watch out for, and
           the ordinary case is not one — labelling it protests too much. */
        null
      ) : money === "stranger" ? (
        <span className="badge bad" title="The ledger answered; it does not know this browser's token.">
          token not recognised
        </span>
      ) : (
        <span className="badge bad" title="There is a ledger, and this page cannot reach it. Nothing shown is a balance.">
          ledger unreachable
        </span>
      )}
    </header>
  );
}
