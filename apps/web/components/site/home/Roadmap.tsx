import { ROADMAP } from "../../../lib/home";
import { Section } from "../Section";

/** #roadmap: what's next, numbered and labelled as not in the download. */
export function Roadmap() {
  return (
    <Section
      id="roadmap"
      eyebrow="Roadmap"
      title="What we're building next"
      lead="None of this is in the download yet. It's the order we're working in."
    >
      <ol className="grid gap-px overflow-hidden rounded-md border border-border bg-border md:grid-cols-3">
        {ROADMAP.map((r, i) => (
          <li key={r.title} className="reveal flex flex-col gap-3 bg-page p-6">
            <span className="font-mono text-caption text-fg-subtle">Next · {String(i + 1).padStart(2, "0")}</span>
            <h3 className="text-title text-fg">{r.title}</h3>
            <p className="text-copy-sm text-fg-muted">{r.body}</p>
          </li>
        ))}
      </ol>
    </Section>
  );
}
