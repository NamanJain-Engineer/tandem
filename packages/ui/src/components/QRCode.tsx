import { cx } from "../cx";

export type QRCodeProps = {
  /** One string per module row, `1` dark and `0` light (the app's `QrMatrix.rows`). */
  rows: string[];
  /** What the code is for, for screen readers ("Pairing code"). */
  label: string;
  /** Rendered width and height in px. */
  size?: number;
  className?: string;
};

/** UI_SPEC §2.31 `QRCode`: dark modules on a light panel with a 4-module
 * quiet zone in both themes, since phone cameras read light-on-dark codes
 * poorly. One path, so large codes stay cheap to render. */
export function QRCode({ rows, label, size = 224, className }: QRCodeProps) {
  const n = rows.length;
  const quiet = 4;
  const view = n + quiet * 2;
  let d = "";
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) {
      if (row[x] === "1") d += `M${x + quiet} ${y + quiet}h1v1h-1z`;
    }
  });
  return (
    <svg
      role="img"
      aria-label={label}
      width={size}
      height={size}
      viewBox={`0 0 ${view} ${view}`}
      shapeRendering="crispEdges"
      className={cx("rounded-md", className)}
    >
      <rect width={view} height={view} fill="var(--slate-50)" />
      <path d={d} fill="var(--slate-950)" />
    </svg>
  );
}
