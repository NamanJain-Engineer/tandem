/** Vibezeo's mark: two overlapping rings. `glow` adds the video poster's soft
 * teal glow (the hero and the closing call to action). */
export function Mark({ size = 22, glow = false }: { size?: number; glow?: boolean }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 28 28"
      fill="none"
      aria-hidden="true"
      className={glow ? "drop-shadow-[0_0_24px_var(--site-glow)]" : undefined}
    >
      <circle cx="10.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
      <circle cx="17.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
    </svg>
  );
}
