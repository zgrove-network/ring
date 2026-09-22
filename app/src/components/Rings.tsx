import { TARGET_SECONDS, type Block } from "../lib/chain";

/** One ring per block, and the ring's width is how long that block took — in
 * a trunk a wide ring is a long season, here a long wait.
 *
 * The rings a chosen miner found are lit. Picking one and watching its blocks
 * light up across the trunk shows its cadence: whether it is spread evenly or
 * arrives in runs, which is the thing a published 24-hour share figure
 * flattens away. */
export function Rings({
  blocks,
  picked,
}: {
  readonly blocks: readonly Block[];
  readonly picked: string | null;
}) {
  const ordered = [...blocks].slice(0, 60).reverse().filter((b) => b.interval !== null);
  if (ordered.length === 0) return null;

  const gaps = ordered.map((b) => b.interval ?? 0);
  const total = gaps.reduce((a, b) => a + b, 0) || 1;

  const HEART = 22;
  const BUDGET = 440;
  const MIN_STEP = 2;
  const spare = Math.max(BUDGET - MIN_STEP * ordered.length, 0);

  let r = HEART;
  const rings = ordered.map((block, i) => {
    r += MIN_STEP + ((gaps[i] ?? 0) / total) * spare;
    return {
      block,
      r,
      lit: picked !== null && block.miner === picked,
      slow: (block.interval ?? 0) >= TARGET_SECONDS,
    };
  });

  return (
    <svg
      className="rings"
      viewBox="0 0 460 320"
      preserveAspectRatio="xMaxYMid slice"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="ring-fade" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#000" />
          <stop offset="0.68" stopColor="#3a3a3a" />
          <stop offset="1" stopColor="#d2d2d2" />
        </linearGradient>
        <mask id="ring-mask">
          <rect width="460" height="320" fill="url(#ring-fade)" />
        </mask>
      </defs>

      <g mask="url(#ring-mask)" fill="none">
        {rings.map(({ block, r: radius, lit, slow }) => (
          <circle
            key={block.height}
            cx={520}
            cy={196}
            r={radius}
            stroke={lit ? "var(--good)" : slow ? "var(--ring-lit)" : "var(--ring)"}
            strokeWidth={lit ? 2 : slow ? 1.3 : 0.8}
            opacity={lit ? 0.62 : slow ? 0.3 : 0.2}
          />
        ))}
      </g>
    </svg>
  );
}
