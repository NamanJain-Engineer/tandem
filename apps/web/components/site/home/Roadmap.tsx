import { ROADMAP } from "../../../lib/home";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";

/** #roadmap: what's next, numbered and labelled as not in the download. */
export function Roadmap() {
  return (
    <Section
      id="roadmap"
      divider
      eyebrow="Roadmap"
      title="What we're building next"
      lead="None of this is in the download yet. It's the order we're working in."
    >
      <ol className="surface grid overflow-hidden p-0 md:grid-cols-3">
        {ROADMAP.map((r, i) => (
          <li key={r.title} className={`reveal ${stagger(i)} flex flex-col gap-3 border-line p-6 lg:p-7 [&+&]:max-md:border-t [&+&]:md:border-l`}>
            <span className="font-mono text-[0.6875rem] uppercase tracking-[0.12em] text-fg-subtle">Next · {String(i + 1).padStart(2, "0")}</span>
            <h3 className="text-title text-fg">{r.title}</h3>
            <p className="text-copy-sm text-fg-muted">{r.body}</p>
          </li>
        ))}
      </ol>
    </Section>
  );
}
