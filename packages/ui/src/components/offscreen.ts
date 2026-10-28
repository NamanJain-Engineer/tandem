import type { CSSProperties } from "react";

/** A row of a long list that skips layout and paint while it's off screen
 * (`content-visibility: auto`), keeping `height` (its usual height, in px)
 * as its size until it has been laid out once, then its last real size.
 * The row is paint-contained and keeps its own layer; what's inside it adds
 * none while it's off screen. */
export function offscreenRow(height: number): CSSProperties {
  return { contentVisibility: "auto", containIntrinsicSize: `auto ${height}px` };
}
