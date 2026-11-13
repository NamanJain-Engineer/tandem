/** The light and lit edge inside a `data-lit` surface (D-2576): place it as
 * the surface's first child. Decorative, and inert without the pointer. */
export function LitGlow() {
  return (
    <span aria-hidden="true" className="lit-glow">
      <span className="lit-edge" />
    </span>
  );
}
