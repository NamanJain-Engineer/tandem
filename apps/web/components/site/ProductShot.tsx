import { cx } from "../../lib/cx";

export type Shot = {
  /** The PNG or JPEG fallback, under /public. */
  src: string;
  width: number;
  height: number;
  alt: string;
  /** Modern formats beside `src` (same name, other extension). */
  formats?: ReadonlyArray<"avif" | "webp">;
};

/** A real capture of the app on the dark stage, with a caption. Lazy below
 * the fold, which also stops React preloading it. */
export function ProductShot({
  shot,
  caption,
  sizes = "(min-width: 1280px) 1200px, 100vw",
  eager = false,
  zoom = false,
  className,
}: {
  shot: Shot;
  caption?: string;
  sizes?: string;
  eager?: boolean;
  /** Link the image to its full-size file. */
  zoom?: boolean;
  className?: string;
}) {
  const base = shot.src.replace(/\.(png|jpe?g)$/, "");
  const img = (
    <picture>
      {(shot.formats ?? []).map((f) => (
        <source key={f} type={`image/${f}`} srcSet={`${base}.${f}`} sizes={sizes} />
      ))}
      <img
        src={shot.src}
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
    <figure className={cx("reveal flex flex-col gap-3", className)}>
      <div data-stage="" className="overflow-hidden rounded-lg bg-stage shadow-stage">
        {zoom ? (
          <a href={shot.src} title="Open full size" className="block">
            {img}
          </a>
        ) : (
          img
        )}
      </div>
      {caption && <figcaption className="text-caption text-pretty text-fg-muted">{caption}</figcaption>}
    </figure>
  );
}
