import { Fragment, memo, useMemo, useState, type ReactNode } from "react";
import { Check, Copy, Square, SquareCheck } from "lucide-react";
import { cx } from "../cx";
import { MARKDOWN_MAX, parseMarkdown, splitStable, type Block, type Inline } from "./markdown";

export type CodeBlockProps = {
  code: string;
  language?: string;
  copyLabel?: string;
  copiedLabel?: string;
  className?: string;
};

/** Mono block with Copy and an optional language label (§2.31). */
export function CodeBlock({ code, language, copyLabel = "Copy", copiedLabel = "Copied", className }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard?.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };
  return (
    <div className={cx("group relative overflow-hidden rounded-md border border-border bg-surface-1", className)}>
      <div className="flex h-7 items-center justify-between border-b border-divider px-2 text-2xs text-fg-subtle">
        <span className="font-mono">{language || " "}</span>
        <button
          type="button"
          onClick={copy}
          aria-label={copied ? copiedLabel : copyLabel}
          className="inline-flex items-center gap-1 rounded-xs px-1 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          {copied ? <Check size={12} aria-hidden="true" /> : <Copy size={12} aria-hidden="true" />}
          {copied ? copiedLabel : copyLabel}
        </button>
      </div>
      <pre className="overflow-x-auto p-3 font-mono text-sm leading-relaxed text-fg">
        <code>{code}</code>
      </pre>
    </div>
  );
}

export type MarkdownViewProps = {
  markdown: string;
  /** While text streams in: only the last block re-parses. */
  streaming?: boolean;
  /** Called for every link click; the app opens it (with a confirm for
   * non-https). Links never navigate the webview. */
  onLink?: (href: string) => void;
  copyLabel?: string;
  copiedLabel?: string;
  /** Task list items: "Done" / "To do". */
  doneLabel?: string;
  todoLabel?: string;
  className?: string;
};

function inlines(nodes: Inline[], onLink: ((href: string) => void) | undefined): ReactNode {
  return nodes.map((n, i) => {
    switch (n.t) {
      case "text":
        return <Fragment key={i}>{n.v}</Fragment>;
      case "code":
        return (
          <code key={i} className="rounded-xs bg-surface-3 px-1 py-px font-mono text-sm">
            {n.v}
          </code>
        );
      case "strong":
        return (
          <strong key={i} className="font-semibold">
            {inlines(n.c, onLink)}
          </strong>
        );
      case "em":
        return <em key={i}>{inlines(n.c, onLink)}</em>;
      case "del":
        return <del key={i}>{inlines(n.c, onLink)}</del>;
      case "br":
        return <br key={i} />;
      case "link":
        return (
          <a
            key={i}
            href={n.href}
            onClick={(e) => {
              e.preventDefault();
              onLink?.(n.href);
            }}
            className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
          >
            {inlines(n.c, onLink)}
          </a>
        );
    }
  });
}

const HEADING = ["text-xl", "text-lg", "text-md", "text-base", "text-base", "text-base"];

type BlockProps = Required<Pick<MarkdownViewProps, "copyLabel" | "copiedLabel" | "doneLabel" | "todoLabel">> & {
  onLink?: (h: string) => void;
};

function blocks(list: Block[], p: BlockProps): ReactNode {
  return list.map((b, i) => {
    switch (b.t) {
      case "heading": {
        const Tag = `h${b.level}` as "h1";
        return (
          <Tag key={i} className={cx("mt-2 font-semibold text-fg", HEADING[b.level - 1])}>
            {inlines(b.c, p.onLink)}
          </Tag>
        );
      }
      case "para":
        return (
          <p key={i} className="whitespace-pre-wrap">
            {inlines(b.c, p.onLink)}
          </p>
        );
      case "code":
        return <CodeBlock key={i} code={b.v} language={b.lang} copyLabel={p.copyLabel} copiedLabel={p.copiedLabel} />;
      case "quote":
        return (
          <blockquote key={i} className="flex flex-col gap-2 border-l-2 border-border-strong pl-3 text-fg-muted">
            {blocks(b.blocks, p)}
          </blockquote>
        );
      case "hr":
        return <hr key={i} className="border-divider" />;
      case "list": {
        const items = b.items.map((it, n) => (
          <li key={n} className={cx("flex flex-col gap-1", it.checked !== null && "list-none")}>
            {it.checked !== null ? (
              <span className="flex items-start gap-2">
                {it.checked ? (
                  <SquareCheck size={14} role="img" aria-label={p.doneLabel} className="mt-1 shrink-0 text-accent" />
                ) : (
                  <Square size={14} role="img" aria-label={p.todoLabel} className="mt-1 shrink-0 text-fg-subtle" />
                )}
                <span className="flex min-w-0 flex-col gap-1">{blocks(it.blocks, p)}</span>
              </span>
            ) : (
              blocks(it.blocks, p)
            )}
          </li>
        ));
        return b.ordered ? (
          <ol key={i} start={b.start} className="flex list-decimal flex-col gap-1 pl-6">
            {items}
          </ol>
        ) : (
          <ul key={i} className="flex list-disc flex-col gap-1 pl-6">
            {items}
          </ul>
        );
      }
      case "table":
        return (
          <div key={i} className="overflow-x-auto">
            <table className="min-w-full border-collapse text-sm">
              <thead>
                <tr>
                  {b.head.map((h, n) => (
                    <th key={n} style={{ textAlign: b.align[n] ?? "left" }} className="border-b border-border px-2 py-1 font-semibold">
                      {inlines(h, p.onLink)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {b.rows.map((r, n) => (
                  <tr key={n} className="border-b border-divider">
                    {r.map((c, m) => (
                      <td key={m} style={{ textAlign: b.align[m] ?? "left" }} className="px-2 py-1 align-top">
                        {inlines(c, p.onLink)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
    }
  });
}

/** The settled part of a streaming message, parsed once per change. */
const Stable = memo(function Stable({ src, p }: { src: string; p: BlockProps }) {
  const parsed = useMemo(() => parseMarkdown(src), [src]);
  return <>{blocks(parsed, p)}</>;
});

/** Agent messages (§2.31): GFM subset, never raw HTML. */
export function MarkdownView({
  markdown,
  streaming = false,
  onLink,
  copyLabel = "Copy",
  copiedLabel = "Copied",
  doneLabel = "Done",
  todoLabel = "To do",
  className,
}: MarkdownViewProps) {
  const p = useMemo(
    () => ({ copyLabel, copiedLabel, doneLabel, todoLabel, onLink }),
    [copyLabel, copiedLabel, doneLabel, todoLabel, onLink],
  );
  const tooBig = markdown.length > MARKDOWN_MAX;
  const { stable, tail } = streaming ? splitStable(markdown) : { stable: markdown, tail: "" };
  return (
    <div className={cx("flex flex-col gap-3 text-md leading-relaxed text-fg", className)} data-streaming={streaming || undefined}>
      {tooBig ? (
        <p className="whitespace-pre-wrap font-mono text-sm">{markdown}</p>
      ) : (
        <>
          <Stable src={stable} p={p} />
          {tail && blocks(parseMarkdown(tail), p)}
        </>
      )}
    </div>
  );
}
