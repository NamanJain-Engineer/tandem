import { cx } from "../../lib/cx";
import { SITE } from "../../lib/site";

export type Shot = {
  /** The PNG or JPEG fallback, under /public. */
  src: string;
  width: number;
  height: number;
  alt: string;
  /** Modern formats beside `src` (same name, other extension). */
  formats?: ReadonlyArray<"avif" | "webp">;
  /** A twice-as-sharp capture beside it, named `<name>@2x.<ext>` (D-1838). */
  retina?: boolean;
  /** The build the capture shows: every capture names it (H1, D-1834). */
  build: string;
};

/** A real capture of the app in the site's bezel, labelled with the build it
 * shows, with a caption. Lazy below the fold, which also stops React
 * preloading it. `fade` lets the bottom of a tall capture melt into the page. */
export function ProductShot({
  shot,
  caption,
  sizes = "(min-width: 1280px) 1200px, 100vw",
  eager = false,
  zoom = false,
  fade = false,
  className,
}: {
  shot: Shot;
  caption?: string;
  sizes?: string;
  eager?: boolean;
  /** Link the image to its full-size file. */
  zoom?: boolean;
  fade?: boolean;
  className?: string;
}) {
  const base = shot.src.replace(/\.(png|jpe?g)$/, "");
  const ext = shot.src.slice(base.length + 1);
  const set = (e: string) =>
    shot.retina ? `${base}.${e} ${shot.width}w, ${base}@2x.${e} ${shot.width * 2}w` : `${base}.${e}`;
  const img = (
    <picture>
      {(shot.formats ?? []).map((f) => (
        <source key={f} type={`image/${f}`} srcSet={set(f)} sizes={sizes} />
      ))}
      <img
        src={shot.src}
        srcSet={shot.retina ? set(ext) : undefined}
        sizes={shot.retina ? sizes : undefined}
        width={shot.width}
        height={shot.height}
        alt={shot.alt}
        loading={eager ? "eager" : "lazy"}
        decoding="async"
        className="block h-auto w-full"
      />
    </picture>
  );
  return (
    <figure data-capture="" data-build={shot.build} className={cx("rise flex flex-col gap-4", className)}>
      <div className={cx("frame", fade && "[mask-image:linear-gradient(to_bottom,black_70%,transparent)]")}>
        <div data-stage="" className="frame-screen">
          {zoom ? (
            <a href={shot.src} title="Open full size" className="block">
              {img}
            </a>
          ) : (
            img
          )}
        </div>
      </div>
      <figcaption className="flex flex-col gap-1.5 text-caption text-pretty text-fg-muted sm:flex-row sm:items-baseline sm:gap-4">
        <span className="shrink-0 font-mono text-[0.6875rem] uppercase tracking-[0.1em] text-fg-subtle">
          {SITE.product} {shot.build} · Linux
        </span>
        {caption && <span>{caption}</span>}
      </figcaption>
    </figure>
  );
}
