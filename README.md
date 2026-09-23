# ring

A market on Zcash blocks, and the wallet that would read positions out of the
chain.

Ring is the companion to [zGrove](https://github.com/zgrove-network/zgrove),
the mining pool that pays contributors in shielded ZEC. The pool is for people
with a GPU. This is for everyone else.

## What is here

```
app/      the market — Vite, React, reads live Zcash blocks
wallet/   the wallet positions would be sent to — Rust, librustzcash
docs/     what the sealed-bid auction that came before this taught
```

## What is real, and what is not

Say this plainly, because a screen showing numbers is easy to mistake for a
thing that works.

**Real:** the chain data. Blocks, which miner took each one, the interval
between them — all live from a Zcash explorer. Nothing about the chain is
invented.

**Not real:** the market. A balance starts at 1 ZEC and lives in React state.
Taking a position sets a variable. No wallet is involved, no memo is
broadcast, and no money moves anywhere. It is a simulation running on real
data.

**Started:** the wallet. `wallet/` generates the account positions would be
sent to — a seed that can spend, a viewing key that can only read, and an
address to publish. The seed is written on paper and never reaches a server;
the viewing key is the most a machine on the internet is allowed to hold, so
that breaking into it leaks what people staked without moving it.

## What a real position would take

Positions have to arrive as shielded payments carrying a memo, because the
whole point is that nobody can see who staked what. That has consequences
which are not obvious:

- The sender is not on the chain and cannot be, so the **memo has to carry the
  return address itself**, or a winner cannot be paid.
- Memos are **not in compact blocks**. A compact block carries the first 52
  bytes of a note's ciphertext — enough to spot the note and read its value,
  not enough to read its memo. Every detected transaction has to be fetched in
  full and decrypted again.
- `zcash_client_backend::sync::run` cannot be used as it stands: it wants a
  `BlockCache`, and no published crate implements that trait. That layer has
  to be written here.

## Running the market

```sh
cd app && pnpm install && pnpm dev
```

## The wallet

```sh
cd wallet && cargo run -- new
```

Prints a seed phrase, a viewing key and an address. Generate it on a machine
you trust; the seed is the only thing that can spend, and nothing here should
ever see it again.
