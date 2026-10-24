import type { ReactNode } from "react";
import { cx } from "../../../lib/cx";
import { SCENE_BUILD } from "../../../lib/scenes";
import { SITE } from "../../../lib/site";

/** A product scene in the site's bezel (D-1834, D-1880), labelled as a
 * simplified scene of the build it shows. The scene itself is decorative
 * (`aria-hidden`); its caption says what happens in it. `/demo/embed.js`
 * plays it while it's on screen and the tab is visible. */
export function Scene({
  id,
  text,
  caption,
  children,
  className,
}: {
  id: string;
  /** What happens in the scene, for people who can't see it. */
  text: string;
  /** A short visible caption; `text` is read instead where it's given. */
  caption?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <figure data-scene={id} className={cx("flex flex-col gap-4", className)}>
      <div className="frame">
        <div data-stage="" aria-hidden="true" className="frame-screen scene-box">
          {children}
        </div>
      </div>
      <figcaption className="flex flex-col gap-1.5 text-caption text-pretty text-fg-muted sm:flex-row sm:items-baseline sm:gap-4">
        <span className="shrink-0 font-mono text-[0.6875rem] uppercase tracking-[0.1em] text-fg-subtle">
          Scene from {SITE.product} {SCENE_BUILD}, simplified
        </span>
        {caption ? (
          <>
            <span aria-hidden="true">{caption}</span>
            <span className="sr-only">{text}</span>
          </>
        ) : (
          <span>{text}</span>
        )}
      </figcaption>
    </figure>
  );
}
