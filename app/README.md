# ring

A market on which miner takes the next Zcash block, where nobody can see how
the money is spread until it lands.

```
pnpm --filter @zgrove/ring dev
```

## Why this question

Two things had to be true at once, and almost nothing satisfies both.

**Moving the odds has to be expensive.** Zcash's network earns about $2.4M a
day, so ten points of share costs roughly $240,000 a day of hashrate — and
every day after that, because the moment you stop, your share falls back. The
alternative we looked at first, a market on the size of the shielded pool,
could be shoved around for a fifteen cent transaction fee by anyone holding a
few hundred ZEC, and got easier to attack the more money the market held.

**Somebody has to be able to know something.** A market on block intervals is
unriggable but solved: intervals are exponential around the 75 second target,
about 63% land inside it, and once that is written down nobody knows anything
anyone else does not. Sealing a book where no one has an edge protects
nothing.

Here they do. Published pool shares are computed over 24-hour windows, so a
large miner moving between pools shows up in the block stream hours before any
dashboard reflects it. Watching the chain beats reading a table — which is the
shape of a real edge, and the reason the book is worth sealing: you can act on
what you saw without announcing it.

## Timing

There is no betting window and no closing bell, because a position does not
choose its round. You broadcast a memo whenever you like; the chain decides
which block carries it; it settles on the block after that one.

```
broadcast     memo sent
carried by    block C          <- the chain picked this, not you
settles on    block C+1        <- does not exist yet
```

Two things fall out of that. The block a bet settles on had not been mined
when the bet was written down, so nobody — bettor, house or miner — can have
seen the result. And last-second sniping is impossible, because nobody picks
which block their memo lands in.

One round per block: about 48 an hour, against a Polymarket market that runs
for weeks. The cost is that you do not know which round you have joined until
your memo lands.

## What is real and what is not

**Real:** every block, every interval, and who took it. Miners are read from
the coinbase, which is where pools write their own names.

**Not real:** the counterparties. The other side of each pool is simulated and
the top bar says so.

## unsigned

About half of blocks carry no recognisable name, and that lines up with the
two largest pools by published share, which do not sign their coinbase. The
interface does not claim it is them. The honest label is that nobody signed
it, which is a fact anyone can check, and it is an outcome you can back like
any other. Splitting it further means building an address-to-pool map by
observation — worth doing, not done here.

## The rings

One per block, width equal to how long that block took. Backing a miner lights
the rings it found, which shows whether its blocks are spread evenly or arrive
in runs — the thing a 24-hour share figure flattens away.
