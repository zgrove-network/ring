/** The zGrove mark: a Z whose stem forks into roots. Strokes take
 * currentColor so it sits in whatever it is placed in. */
export function Mark({ size = 18 }: { readonly size?: number }) {
  return (
    <svg
      className="mark"
      width={size}
      height={size * (226 / 184)}
      viewBox="42 20 184 226"
      role="img"
      aria-label="zGrove"
    >
      <g
        fill="none"
        stroke="currentColor"
        strokeWidth={30}
        strokeLinecap="butt"
        strokeLinejoin="miter"
        strokeMiterlimit={10}
      >
        <path d="M50,35 H190 L70,155" />
        <path d="M42,155 H142 V189" />
        <path d="M105.9,236.9 L142,189 L178.1,236.9" />
      </g>
    </svg>
  );
}
