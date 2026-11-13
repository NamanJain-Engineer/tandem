/** The light and lit edge inside a `data-lit` surface (D-2576): place it as
 * the surface's first child. Decorative; public/site.js moves the light with
 * the pointer, and without it the surface stays unlit. */
export function LitGlow() {
  return (
    <span aria-hidden="true" className="lit-glow">
      <span className="lit-edge" />
    </span>
  );
}
