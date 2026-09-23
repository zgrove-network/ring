import { useState } from "react";

import type { Money } from "../hooks/useMarket";
import { signed, zec } from "../lib/market";

interface Props {
  readonly money: Money;
  readonly depositAddress: string | null;
  readonly network: string | null;
  readonly unit: string;
  readonly cashOut: (zatoshi: number, to: string) => Promise<void>;
  readonly startOver: () => Promise<void>;
  readonly balance: number;
  readonly session: number;
  readonly hits: number;
  readonly staked: number;
}

/** The address, and a way to take it.
 *
 * It is a couple of hundred characters of base32 and nobody is going to
 * retype it. Showing it without a way to copy it is showing it for decoration.
 */
function Deposit({ address }: { address: string }) {
  const [took, setTook] = useState(false);
  return (
    <div className="deposit">
      <p className="note addr-line" title={address}>
        {address.slice(0, 22)}…{address.slice(-8)}
      </p>
      <button
        type="button"
        className="copy"
        onClick={() => {
          void navigator.clipboard?.writeText(address).then(
            () => {
              setTook(true);
              setTimeout(() => setTook(false), 1500);
            },
            () => undefined,
          );
        }}
      >
        {took ? "copied" : "copy address"}
      </button>
    </div>
  );
}

/** Asking for money back.
 *
 * Nothing here takes a deposit without showing the way out on the same
 * panel. The payment is made by hand from a machine that holds the spending
 * key, so this says so rather than implying a transfer that is already on
 * its way.
 */
function CashOut({
  unit,
  balance,
  cashOut,
}: {
  unit: string;
  balance: number;
  cashOut: (zatoshi: number, to: string) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const [state, setState] = useState<"idle" | "sending" | "asked">("idle");
  const [why, setWhy] = useState<string | null>(null);

  if (!open) {
    return (
      <button type="button" className="cash-out" onClick={() => setOpen(true)}>
        withdraw
      </button>
    );
  }

  return (
    <form
      className="cash-out-form"
      onSubmit={(event) => {
        event.preventDefault();
        const zatoshi = Math.round(Number(amount) * 1e8);
        if (!Number.isFinite(zatoshi) || zatoshi <= 0) {
          setWhy("that is not an amount");
          return;
        }
        setState("sending");
        setWhy(null);
        void cashOut(zatoshi, to.trim()).then(
          () => setState("asked"),
          (error: unknown) => {
            setState("idle");
            setWhy(error instanceof Error ? error.message : "the ledger refused it");
          },
        );
      }}
    >
      <label htmlFor="cash-to">your shielded address</label>
      <input
        id="cash-to"
        value={to}
        onChange={(e) => setTo(e.target.value)}
        placeholder="u1..."
        autoComplete="off"
        spellCheck={false}
      />

      <label htmlFor="cash-amount">amount ({unit})</label>
      <input
        id="cash-amount"
        value={amount}
        onChange={(e) => setAmount(e.target.value)}
        placeholder={balance.toFixed(4)}
        inputMode="decimal"
        autoComplete="off"
      />

      {why === null ? null : <p className="note warn">{why}</p>}

      {state === "asked" ? (
        <p className="note">
          Asked for. The balance is already down by it. Payments are made by
          hand, so this is not on its way yet — you will see the transaction
          when it is.
        </p>
      ) : (
        <button type="submit" disabled={state === "sending" || to.trim() === ""}>
          {state === "sending" ? "asking…" : "ask for it back"}
        </button>
      )}
    </form>
  );
}

export function Account({ money, depositAddress, network, unit, cashOut, startOver, balance, session, hits, staked }: Props) {
  return (
    <section className="panel account">
      <h2>account</h2>
      <dl className="rows">
        <dt>balance</dt>
        <dd className="bright">{zec(balance)}</dd>
        <dt>session</dt>
        <dd className={session === 0 ? undefined : session > 0 ? "good" : "bad"}>
          {signed(session)}
        </dd>
        <dt>called right</dt>
        <dd>
          {hits}/{staked}
        </dd>
      </dl>

      {money === "ledger" && balance > 0 ? (
        <CashOut unit={unit} balance={balance} cashOut={cashOut} />
      ) : null}

      {money === "simulated" ? (
        /* A balance beside live chain data reads as an account somebody is
           holding for you. In this mode nobody is. */
        <p className="note">
          Play money. This figure lives in your browser and nowhere else — no
          wallet is connected and nothing is ever sent.
        </p>
      ) : money === "unreachable" ? (
        <p className="note">
          The ledger did not answer, so this is not a balance. It says nothing
          about whether your money is there.
        </p>
      ) : money === "stranger" ? (
        <>
          <p className="note warn">
            The ledger answered and does not know the token this browser is
            holding. It is not down — this token is not one of its.
          </p>
          <p className="note">
            Starting over takes a new address. Anything the old token held
            cannot be reached from here.
          </p>
          <button type="button" className="cash-out" onClick={() => void startOver()}>
            start over
          </button>
        </>
      ) : (
        <>
          {/* The unit is whatever the ledger says it is. Printing "ZEC"
              beside a testnet address would be an instruction to destroy
              money, and the address alone does not say it loudly enough. */}
          {network === "testnet" ? (
            <p className="note warn">
              Testnet. This address takes <b>TAZ</b>, which is worth nothing.
              Real ZEC sent here is gone.
            </p>
          ) : (
            <p className="note">Send {unit} here to play. Shielded, memo not needed.</p>
          )}
          {depositAddress === null ? null : <Deposit address={depositAddress} />}
        </>
      )}
    </section>
  );
}
