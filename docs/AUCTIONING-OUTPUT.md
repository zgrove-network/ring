# Auctioning the pool's output, and why the arithmetic refused

Figures read on 21 September 2026. Prices move; the shape of the argument does
not, and the shape is the reason this is written down rather than re-derived.

## The idea

Twenty slots per round, one round per block. Bids arrive as shielded memos, so
nobody can read them — not the other bidders, not an observer, and there is no
commit-reveal because a memo is private when it lands. Highest twenty bids take
a slot at the price each one named. The box holds whatever the pool mined
during that block.

Contributors would sell their variance; bidders would buy it. A miner has
always wanted to be paid the same whether or not today was lucky, and here the
party absorbing the luck is a market rather than an operator's balance sheet.
The mechanism holds up. What follows is why it was not built.

## Renting hashrate costs more than the hashrate produces

This is the whole of it. Marketplaces exist because buyers pay a premium over
what mining yields; that premium is the business. Auctioning the output means
paying it and then needing bidders to pay it back with interest.

| | rent, per TH/day | mining value, per TH/day | margin |
|---|---|---|---|
| Ravencoin — NiceHash | $7,300 – $12,350 | $5,256 | −39% to −135% |
| Ravencoin — MiningRigRentals | $14,415 | $5,256 | −174% |
| Ergo — NiceHash | $1,033 | $1,157 | **+12%** |

Ergo is the only one that clears, and it clears by twelve percent. Working:
556.3 GH/s of network, 720 blocks a day at 3 ERG, ERG at $0.2981 — $644 a day
across 0.5563 TH/s. Rent at 0.0123 BTC/TH/day with BTC at $84,000.

Below about **$0.27 per ERG the margin inverts** and there is nothing left
anywhere.

## And Ergo's supply caps out at forty dollars a day

The whole NiceHash Autolykos market is 0.0393 TH/s — $41 a day, 7.1% of the
network, about 51 blocks a day. There is no way to spend more, and no way to
grow. Ravencoin has the room (141 GH/s listed, 18% of its network, and
$5.71M of daily volume to sell into against Ergo's $184K) and loses money per
unit.

## The prize is smaller than the fee to bid on it

At that scale a round's box holds one block — 3 ERG, about $0.89 — and a
twentieth of it is **four and a half cents**. A shielded transaction costs the
ZIP 317 floor of 0.0001 ZEC, which with ZEC at $1,465 is **fifteen cents**.

Bidding costs three times the prize. Nothing about the mechanism fixes that.

## More hashrate does not help

The trap worth remembering: a block reward is fixed, so hashrate buys
**frequency, not size**. Ten times the hashrate is ten times as many boxes of
exactly the same four and a half cents. The levers that actually change the
prize are fewer slots, longer rounds, or a coin whose block is worth more in
dollars — and the coins with large dollar blocks are ASIC-mined, which shuts
out every GPU contributor the pool exists for.

## What would have to be true

- Contributors rather than rentals, so the input is free and the sign of the
  margin stops mattering.
- Enough of them that a round's box is worth bidding on: roughly seven hundred
  cards before the prize clears the fee with room to spare.
- Which is the same thing as saying this is a product for a pool that already
  works, not a way to make one work.

## Sources

- NiceHash marketplace, Autolykos and KawPow order books
- MiningRigRentals, KawPow rigs
- minerstat, 2cryptocalc — ERG and RVN network figures
- ZIP 317, transaction fees
