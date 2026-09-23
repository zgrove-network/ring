import { useState } from "react";

import { signed, zec } from "../lib/market";

interface Props {
  readonly money: "simulated" | "ledger" | "unreachable";
  readonly depositAddress: string | null;
  readonly network: string | null;
  readonly unit: string;
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

export function Account({ money, depositAddress, network, unit, balance, session, hits, staked }: Props) {
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
