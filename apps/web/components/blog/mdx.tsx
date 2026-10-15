import { Copy } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { cx } from "../../lib/cx";

// How a post's Markdown renders (F-WB-06): the site's type and tokens, in a
// 68-character column. Server components only. Code is highlighted at build
// time by fumadocs-mdx's Shiki, in a light and a dark theme; the CSS below
// picks the one matching the visitor's OS theme, as the site does (D-1801).

/** Styles the body's elements; on the wrapper so they reach what MDX emits. */
export const PROSE = cx(
  "text-copy text-fg-muted [font-size:1.0625rem] [line-height:1.75]",
  "[&>*+*]:mt-6",
  "[&_strong]:font-semibold [&_strong]:text-fg",
  // GFM footnotes: small superscript links, and the notes in a quiet block at the end
  "[&_sup>a]:ml-0.5 [&_sup>a]:font-mono [&_sup>a]:text-[0.75em] [&_sup>a]:text-accent [&_sup>a]:no-underline",
  "[&_[data-footnotes]]:!mt-14 [&_[data-footnotes]]:border-t [&_[data-footnotes]]:border-border [&_[data-footnotes]]:pt-6 [&_[data-footnotes]]:text-copy-sm [&_[data-footnotes]_h2]:sr-only",
  "[&_kbd]:rounded-sm [&_kbd]:border [&_kbd]:border-border-strong [&_kbd]:bg-surface-2 [&_kbd]:px-1.5 [&_kbd]:py-px [&_kbd]:font-mono [&_kbd]:text-[0.8125em] [&_kbd]:text-fg [&_kbd]:shadow-[inset_0_-1px_0_var(--tn-border-strong)]",
  // inline code, not code blocks
  "[&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:border [&_:not(pre)>code]:border-border [&_:not(pre)>code]:bg-surface-2 [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-px [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-[0.875em] [&_:not(pre)>code]:text-fg",
);

type Props<T extends keyof React.JSX.IntrinsicElements> = ComponentProps<T>;

/** h2 and h3 carry their id (fumadocs-mdx) and link to themselves. */
function heading(Tag: "h2" | "h3" | "h4", className: string) {
  return function Heading({ id, children, className: _c, ...rest }: Props<typeof Tag>) {
    return (
      <Tag id={id} {...rest} className={cx("group scroll-mt-24 text-balance text-fg", className)}>
        {id ? (
          <a href={`#${id}`} className="no-underline">
            {children}
            <span aria-hidden="true" className="ml-2 text-fg-subtle opacity-0 transition-opacity duration-(--dur-instant) group-hover:opacity-100">
              #
            </span>
          </a>
        ) : (
          children
        )}
      </Tag>
    );
  };
}

/** An image in a post: fixed size, AVIF and WebP when they exist beside it,
 * lazy (a post's images are below its title block), with a caption. */
export function Figure({
  src,
  alt,
  width,
  height,
  caption,
}: {
  src: string;
  alt: string;
  width: number;
  height: number;
  caption?: ReactNode;
}) {
  const base = src.replace(/\.(png|jpe?g)$/, "");
  const modern = base !== src;
  return (
    <figure className="!mt-10 flex flex-col gap-3 [&+*]:!mt-10">
      <div className="overflow-hidden rounded-lg border border-border bg-stage">
        <picture>
          {modern && <source type="image/avif" srcSet={`${base}.avif`} />}
          {modern && <source type="image/webp" srcSet={`${base}.webp`} />}
          <img src={src} alt={alt} width={width} height={height} loading="lazy" decoding="async" className="block h-auto w-full" />
        </picture>
      </div>
      {caption && <figcaption className="text-caption text-pretty text-fg-muted">{caption}</figcaption>}
    </figure>
  );
}

/** A highlighted code block. fumadocs-mdx also hands over an `icon` (an SVG
 * string, a brand logo for some languages), which is dropped; a fence's
 * `title="…"` becomes a file name bar. */
function CodeBlock({
  className,
  style: _style,
  icon: _icon,
  title,
  ...p
}: Props<"pre"> & { icon?: unknown; title?: string }) {
  const pre = (
    <pre
      {...p}
      className={cx(
        className,
        "overflow-x-auto p-5 pr-24 font-mono text-[0.8125rem] leading-6 text-fg",
        "[&_span]:text-(--shiki-dark) [@media(prefers-color-scheme:light)]:[&_span]:text-(--shiki-light)",
      )}
    />
  );
  // The Copy button is in the markup but hidden: /blog/blog.js shows it
  // when the clipboard is there (D-1860). It sits over the block's corner,
  // so showing it moves nothing.
  return (
    <div data-code="" className="group/code relative !my-8 overflow-hidden rounded-md border border-border bg-surface-1">
      {title && (
        <div className="border-b border-border bg-surface-2 px-5 py-2 pr-24 font-mono text-caption text-fg-muted">{title}</div>
      )}
      {pre}
      <button
        type="button"
        data-copy=""
        hidden
        className="absolute right-2 top-2 inline-flex h-8 items-center gap-1.5 rounded-sm border border-border bg-surface-2 px-2.5 text-caption font-medium text-fg-muted opacity-0 transition-opacity duration-(--dur-instant) hover:text-fg focus-visible:opacity-100 group-hover/code:opacity-100 [@media(hover:none)]:opacity-100"
      >
        <Copy size={14} strokeWidth={1.75} aria-hidden="true" focusable="false" xmlns={undefined} />
        <span data-copy-label="">Copy</span>
        <span className="sr-only"> code</span>
      </button>
      <span data-copy-status="" aria-live="polite" className="sr-only" />
    </div>
  );
}

type StaticImage = { src: string; width: number; height: number };

/** `![alt](/blog/<slug>/x.png)`: fumadocs-mdx may hand over the imported
 * file (with its size) or the path. */
function MarkdownImage({ src, alt, width, height }: { src?: string | StaticImage; alt?: string; width?: number | string; height?: number | string }) {
  const s = typeof src === "object" && src ? src : { src: src ?? "", width: Number(width) || 0, height: Number(height) || 0 };
  if (!s.width || !s.height) return <img src={s.src} alt={alt ?? ""} loading="lazy" decoding="async" className="h-auto max-w-full rounded-lg border border-border" />;
  return <Figure src={s.src} alt={alt ?? ""} width={s.width} height={s.height} />;
}

export const BLOG_MDX = {
  h2: heading("h2", "!mt-14 text-display-md"),
  h3: heading("h3", "!mt-10 text-title"),
  h4: heading("h4", "!mt-8 text-copy font-semibold"),
  p: (p: Props<"p">) => <p {...p} className="text-pretty" />,
  a: ({ href = "", ...p }: Props<"a">) => (
    <a
      href={href}
      {...p}
      {...(/^https?:\/\//.test(href) ? { rel: "noopener noreferrer" } : {})}
      className="font-medium text-accent underline decoration-[color-mix(in_oklab,currentColor_40%,transparent)] decoration-1 underline-offset-[0.2em] hover:decoration-current"
    />
  ),
  ul: (p: Props<"ul">) => <ul {...p} className="flex flex-col gap-2 pl-5 [&>li]:list-disc [&>li]:pl-1 [&>li]:marker:text-fg-subtle" />,
  ol: (p: Props<"ol">) => <ol {...p} className="flex flex-col gap-2 pl-5 [&>li]:list-decimal [&>li]:pl-1 [&>li]:marker:font-mono [&>li]:marker:text-caption [&>li]:marker:text-fg-subtle" />,
  li: (p: Props<"li">) => <li {...p} className="[&>ul]:mt-2 [&>ol]:mt-2" />,
  blockquote: (p: Props<"blockquote">) => (
    <blockquote {...p} className="border-l-2 border-accent pl-5 text-fg [&>p]:text-pretty" />
  ),
  hr: () => <hr className="!my-12 border-border" />,
  table: (p: Props<"table">) => (
    <div className="overflow-x-auto rounded-md border border-border">
      <table {...p} className="w-full border-collapse text-left text-copy-sm" />
    </div>
  ),
  th: (p: Props<"th">) => <th {...p} className="border-b border-border bg-surface-2 px-4 py-2.5 font-semibold text-fg" />,
  td: (p: Props<"td">) => <td {...p} className="border-b border-border px-4 py-2.5 align-top [tr:last-child>&]:border-b-0" />,
  pre: CodeBlock,
  img: MarkdownImage,
  Figure,
};
