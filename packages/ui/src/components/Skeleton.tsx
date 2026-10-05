import { useEffect, useState, type ReactNode } from "react";
import { cx } from "../cx";

/** UI_SPEC §2.18: surface-4 blocks with a shimmer (off under reduced motion). */
export function Skeleton({ className, width, height = 12 }: { className?: string; width?: number | string; height?: number }) {
  return <span aria-hidden="true" className={cx("tn-shimmer block rounded-xs bg-surface-4", className)} style={{ width, height }} />;
}

/** True once `active` has been true for `delayMs` (never flash a skeleton for fast loads). */
export function useDelayedFlag(active: boolean, delayMs = 150): boolean {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!active) {
      setShown(false);
      return;
    }
    const t = setTimeout(() => setShown(true), delayMs);
    return () => clearTimeout(t);
  }, [active, delayMs]);
  return shown;
}

export function SkeletonGate({ loading, skeleton, children, label = "Loading" }: { loading: boolean; skeleton: ReactNode; children: ReactNode; label?: string }) {
  const show = useDelayedFlag(loading);
  if (!loading) return <>{children}</>;
  return (
    <div role="status" aria-busy="true" aria-label={label}>
      {show ? skeleton : null}
    </div>
  );
}
