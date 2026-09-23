import { signed, zec } from "../lib/market";

interface Props {
  readonly money: "simulated" | "ledger" | "unreachable";
  readonly depositAddress: string | null;
  readonly balance: number;
  readonly session: number;
  readonly hits: number;
  readonly staked: number;
}

export function Account({ money, depositAddress, balance, session, hits, staked }: Props) {
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
          <p className="note">Send ZEC here to play. Shielded, memo not needed.</p>
          {depositAddress === null ? null : (
            <p className="note addr-line" title={depositAddress}>
              {depositAddress.slice(0, 22)}…{depositAddress.slice(-8)}
            </p>
          )}
        </>
      )}
    </section>
  );
}
