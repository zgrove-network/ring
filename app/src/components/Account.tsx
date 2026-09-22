import { signed, zec } from "../lib/market";

interface Props {
  readonly balance: number;
  readonly session: number;
  readonly hits: number;
  readonly staked: number;
}

export function Account({ balance, session, hits, staked }: Props) {
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
    </section>
  );
}
