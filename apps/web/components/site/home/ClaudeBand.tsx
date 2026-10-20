import { Badge, type BadgeVariant } from "@tandem/ui/components/Badge";
import { CLAUDE } from "../../../lib/home";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";

const TAG: Record<string, BadgeVariant> = {
  Shipped: "done",
  "How we build": "outline",
  "Built, live when accounts open": "waiting",
};

/** #claude (§6.11): what's shipped, how it's built, and what's built but not live. */
export function ClaudeBand() {
  return (
    <Section
      id="claude"
      eyebrow="Built with Claude"
      title="Made for Claude Code, open to every agent"
      align="split"
      lead="Vibezeo gives Claude Code a persistent workspace: run several sessions at once, see which one needs you, and pick up where you left off after a restart, all on your own Claude subscription."
    >
      <ul className="grid gap-4 md:grid-cols-3">
        {CLAUDE.map((c, i) => (
          <li key={c.title} className={`reveal ${stagger(i)} surface flex flex-col gap-4 p-6 lg:p-7`}>
            <Badge variant={TAG[c.tag] ?? "outline"} dot className="self-start">
              {c.tag}
            </Badge>
            <h3 className="text-title text-fg">{c.title}</h3>
            <p className="text-copy-sm text-fg-muted">
              {c.body.split("--resume").flatMap((part, i) => (i === 0 ? [part] : [<code key={i} className="font-mono text-[0.875em] text-fg">--resume</code>, part]))}
            </p>
          </li>
        ))}
      </ul>
    </Section>
  );
}
