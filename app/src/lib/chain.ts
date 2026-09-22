/** Zcash blocks, read from a public explorer.
 *
 * The blocks are real and so is who found them. Nothing about which miner
 * takes the next one is ours to decide, and moving those odds costs hashrate
 * — roughly a tenth of the network is $240,000 a day, every day, against the
 * fifteen cents it would take to shove a shielded-pool number around. That
 * asymmetry is the reason this is the question and not another one. */

const SOURCE = "https://api.blockchair.com/zcash/blocks";

/** Zcash aims for a block every 75 seconds. */
export const TARGET_SECONDS = 75;

/** Blocks whose coinbase carries no recognisable name. This is about half of
 * them, and it lines up with the two largest pools by published share, which
 * do not sign their coinbase. We do not claim it is them: the honest label is
 * that nobody signed it, which is a fact anyone can check. */
export const UNSIGNED = "unsigned";

/** Names that appear inside coinbase data. Matched case-insensitively against
 * the printable bytes, longest first so "2Miners" cannot be eaten by a
 * shorter match. */
const KNOWN = [
  "zecminingpool",
  "milledgeville",
  "HeroMiners",
  "flexpool",
  "NiceHash",
  "2Miners",
  "Foundry",
  "Kryptex",
  "zergpool",
  "AntPool",
  "sluicey",
  "KuPool",
  "F2Pool",
  "ViaBTC",
  "poolin",
  "Luxor",
] as const;

export interface Block {
  readonly height: number;
  /** Epoch milliseconds. */
  readonly at: number;
  /** Seconds since the previous block. Null for the oldest one we hold. */
  readonly interval: number | null;
  readonly miner: string;
}

interface Raw {
  readonly id: number;
  readonly time: string;
  readonly coinbase_data_hex: string | null;
  readonly guessed_miner: string | null;
}

function minerOf(row: Raw): string {
  const guess = row.guessed_miner;
  if (guess !== null && guess !== "" && guess.toLowerCase() !== "unknown") return guess;

  const hex = row.coinbase_data_hex;
  if (hex === null || hex === "") return UNSIGNED;

  let bytes: Uint8Array;
  try {
    bytes = Uint8Array.from(hex.match(/../g) ?? [], (b) => Number.parseInt(b, 16));
  } catch {
    return UNSIGNED;
  }

  const text = Array.from(bytes, (c) => (c >= 32 && c < 127 ? String.fromCharCode(c) : " "))
    .join("")
    .toLowerCase();

  for (const name of KNOWN) if (text.includes(name.toLowerCase())) return name;
  return UNSIGNED;
}

function parse(rows: readonly Raw[]): readonly Block[] {
  const sorted = [...rows].sort((a, b) => a.id - b.id);
  const out: Block[] = [];

  for (let i = 0; i < sorted.length; i += 1) {
    const row = sorted[i];
    if (row === undefined) continue;
    // Blockchair reports UTC without a zone marker.
    const at = Date.parse(`${row.time.replace(" ", "T")}Z`);
    const before = sorted[i - 1];
    const interval =
      before === undefined
        ? null
        : Math.round((at - Date.parse(`${before.time.replace(" ", "T")}Z`)) / 1000);
    out.push({ height: row.id, at, interval, miner: minerOf(row) });
  }

  // Newest first.
  return out.reverse();
}

/** The explorer refuses anything above a hundred, and says so with a 400
 * rather than a short page, so asking for more loses every block. */
export const MAX_LIMIT = 100;

export async function recentBlocks(limit = MAX_LIMIT): Promise<readonly Block[]> {
  const capped = Math.min(limit, MAX_LIMIT);
  const url = `${SOURCE}?limit=${capped}&fields=id,time,coinbase_data_hex,guessed_miner`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`explorer returned ${response.status}`);
  const body: unknown = await response.json();
  const data = (body as { data?: readonly Raw[] }).data;
  if (data === undefined) throw new Error("explorer returned no blocks");
  return parse(data);
}
