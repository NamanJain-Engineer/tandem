import { siteButton } from "@tandem/ui/components/SiteButton";
import { Download } from "lucide-react";
import { PLANS } from "../../../lib/pricing";
import { SITE } from "../../../lib/site";
import { Icon } from "../Icon";
import { Section } from "../Section";
import { TextLink } from "../TextLink";
import { stagger } from "../../../lib/motion";

/** #pricing on the home page: the three plans, divided by hairlines. */
export function PlanTeaser() {
  return (
    <Section
      id="pricing"
      tone="raised"
      eyebrow="Pricing"
      title="Free to run your agents"
      lead={`Free is available today; Pro and Team start with ${SITE.product} accounts. Your agents always run on your own subscriptions.`}
      aside={<TextLink href="/pricing/">Compare the plans</TextLink>}
    >
      <ul className="surface-raised grid overflow-hidden p-0 md:grid-cols-3">
        {PLANS.map((p, i) => (
          <li
            key={p.id}
            className={`reveal ${stagger(i)} relative flex flex-col gap-4 p-6 lg:p-8 ${i > 0 ? "border-t border-line md:border-l md:border-t-0" : ""}`}
          >
            <h3 className="text-title text-fg">{p.name}</h3>
            <p className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
              <span className="text-sheen font-mono text-stat font-medium tabular-nums text-fg">{p.price}</span>
              <span className="text-caption text-fg-muted">{p.cadence}</span>
            </p>
            <p className="flex-1 text-copy-sm text-fg-muted">{p.summary}</p>
            {p.id === "free" ? (
              <a href="/download/" className={siteButton("primary", "md", "self-start")}>
                <Icon icon={Download} size={16} />
                Download
              </a>
            ) : (
              <p className="text-caption text-fg-muted">Starts with {SITE.product} accounts.</p>
            )}
          </li>
        ))}
      </ul>
    </Section>
  );
}
