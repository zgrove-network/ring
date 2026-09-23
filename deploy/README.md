# Running the ledger

It sits on the same machine as the pool and shares nothing with it but Caddy:
no code, no database, no repository. Two services, because a chain sync that
wedges should not take balances offline with it.

```
api      answers the market: join, standing, bet, withdraw
keeper   reads the chain, records deposits, closes rounds the chain decided
```

## What is here and what is not

The **viewing key** is here. It reads every deposit and can spend none of
them, which is the most a box reachable from the internet should hold.

The **spending key is not here and must never be**. Withdrawals are committed
by the API — the balance is debited — and paid by running `ring-wallet pay`
somewhere else, from the seed phrase, by hand. So the worst an intruder does
is learn who is owed what and move balances between rounds. Bad, and not the
same as taking the money.

## Setting it up

```sh
cp .env.example .env     # then fill in RING_UFVK and RING_BIRTHDAY
docker compose up -d --build
```

Caddy is in the pool's compose project, so this joins its network rather than
starting a second one. Route a hostname to `api:5321` there.

## Paying what is owed

From a machine that holds the seed, never from the server:

```sh
ring-wallet pay --seed-file <path> --data <a copy of the ledger>
```

`pay` refuses to build against a wallet that has not been scanned lately: the
expiry height comes from what the wallet believes the tip is, and a stale one
builds a transaction that is already expired.
