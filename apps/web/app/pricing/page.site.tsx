import { siteButton } from "@tandem/ui/components/SiteButton";
import { PRICES } from "@tandem/protocol/plans";
import { Check, Download } from "lucide-react";
import type { Metadata } from "next";
import { Tag } from "../../components/site/Card";
import { CtaBand } from "../../components/site/CtaBand";
import { Icon } from "../../components/site/Icon";
import { PageHero } from "../../components/site/PageHero";
import { PlanComparison } from "../../components/site/PlanComparison";
import { Section } from "../../components/site/Section";
import { SiteFrame } from "../../components/site/SiteFrame";
import { cx } from "../../lib/cx";
import { CREDIT_ROWS, CREDIT_VALUE_USD, PLANS } from "../../lib/pricing";
import { RELEASE } from "../../lib/release";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Pricing", alternates: { canonical: "/pricing/" } };

const notify = (plan: string) =>
  `mailto:${SITE.contactEmail}?subject=${encodeURIComponent(`${SITE.product} ${plan}: tell me when accounts open`)}`;

/** Billing questions, answered only from plans.ts and pricing.ts. */
const BILLING = [
  {
    q: "Is there a trial?",
    a: `Pro has a ${PRICES.pro.trialDays}-day free trial and Team a ${PRICES.team.trialDays}-day one, both with a 14-day money-back window, once ${SITE.product} accounts open.`,
  },
  {
    q: "How is Team billed?",
    a: `Per seat, with ${PRICES.team.minSeats} seats minimum: $${PRICES.team.month / 100} a seat a month, or $${PRICES.team.year / 100} a seat a year.`,
  },
  {
    q: "Do credits pay for my engines?",
    a: `No. Your coding agents run on your own subscriptions. Credits pay only for ${SITE.product}'s hosted features; one credit is $${CREDIT_VALUE_USD.toFixed(3)} of usage.`,
  },
  {
    q: "What does the download include?",
    a: `${SITE.product} ${RELEASE.version} for Linux runs the Free plan on your computer, with no account. It doesn't contact our servers.`,
  },
] as const;

export default function PricingPage() {
  return (
    <SiteFrame current="/pricing/">
      <PageHero
        center
        eyebrow="Plans"
        title={
          <>
            Free to run your agents. <span className="text-fg-muted">Pro when you run a lot.</span>
          </>
        }
        lead={
          <>
            Free is available now: download {SITE.product} {RELEASE.version} for Linux. Pro and Team start with
            Vibezeo accounts. Your coding agents always run on your own subscriptions, so {SITE.product} never
            charges for engine usage.
          </>
        }
      />

      <section aria-label="Plans" className="relative bg-page pb-(--site-section-y)">
        <ul className="container-site grid gap-5 md:grid-cols-3">
          {PLANS.map((plan) => {
            const now = plan.id === "free";
            return (
              <li
                key={plan.id}
                id={plan.id}
                className={cx(
                  "reveal relative isolate flex scroll-mt-(--site-header-h) flex-col gap-7 overflow-hidden p-7 lg:p-8",
                  now ? "surface shadow-e2" : "surface-raised",
                )}
              >
                {now && <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-56" />}
                <div className="flex items-center justify-between gap-3">
                  <h2 className="text-title text-fg">{plan.name}</h2>
                  <Tag className={now ? "text-accent shadow-[inset_0_0_0_1px_var(--tn-accent)]" : undefined}>
                    {now ? "Available now" : "With accounts"}
                  </Tag>
                </div>
                <p className="flex flex-col gap-2">
                  <span className="text-sheen font-mono text-stat font-medium tabular-nums text-fg">{plan.price}</span>
                  <span className="text-caption text-fg-muted">{plan.cadence}</span>
                </p>
                <p className="text-copy-sm text-fg-muted">{plan.summary}</p>
                {now ? (
                  <div className="flex flex-col gap-2">
                    <a href="/download/" className={siteButton("primary", "md", "w-full")}>
                      <Icon icon={Download} size={16} />
                      Download {RELEASE.version}
                    </a>
                    <p className="text-center text-caption text-fg-muted">Available now, for Linux.</p>
                  </div>
                ) : (
                  <div className="flex flex-col gap-2">
                    <a href={notify(plan.name)} className={siteButton("secondary", "md", "w-full")}>
                      Tell me when
                    </a>
                    <p className="text-center text-caption text-fg-muted">
                      Starts with {SITE.product} accounts.{plan.trial ? ` ${plan.trial}.` : ""}
                    </p>
                  </div>
                )}
                <ul className="flex flex-col gap-3 border-t border-line pt-6 text-copy-sm">
                  {plan.features.map((f) => (
                    <li key={f.text} className="flex items-start gap-2.5">
                      <Icon icon={Check} size={16} className="mt-1 shrink-0 text-accent" />
                      <span className={f.planned ? "text-fg-muted" : "text-fg"}>{f.text}</span>
                    </li>
                  ))}
                </ul>
              </li>
            );
          })}
        </ul>
      </section>

      <Section
        id="compare"
        tone="raised"
        eyebrow="Compare"
        title="Every limit, side by side"
        lead="Generated from the same plan definitions the app enforces."
      >
        <PlanComparison />
      </Section>

      <Section
        id="credits"
        eyebrow="Credits"
        title="What credits pay for"
        lead={
          <>
            Credits come with Pro and Team, when {SITE.product} accounts open. They pay only for hosted AI features,
            which run on the Claude API: thread titles, and Dictate&apos;s Enhance and Polish, which switch on with a
            Pro account (the download, on the Free plan, never uses them). One credit is{" "}
            {`$${CREDIT_VALUE_USD.toFixed(3)}`} of usage. They never pay for your engines.
          </>
        }
      >
        <div className="grid gap-10 lg:grid-cols-12 lg:gap-6">
          <table className="surface w-full overflow-hidden text-left text-copy-sm lg:col-span-6">
            <caption className="sr-only">Credits per hosted feature</caption>
            <tbody>
              {CREDIT_ROWS.map((row) => (
                <tr key={row.key} data-credit-price={row.key} className="border-b border-line last:border-0">
                  <th scope="row" className="px-5 py-4 pr-6 font-medium text-fg">
                    {row.label}
                  </th>
                  <td className="px-5 py-4 text-right font-mono tabular-nums text-fg-muted">
                    {row.credits === 0 ? "Free" : `${row.credits} credits ${row.unit}`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <dl className="grid gap-6 sm:grid-cols-2 lg:col-span-6 lg:col-start-7">
            {BILLING.map((b) => (
              <div key={b.q} className="flex flex-col gap-2">
                <dt className="text-copy font-semibold text-fg">{b.q}</dt>
                <dd className="text-copy-sm text-fg-muted">{b.a}</dd>
              </div>
            ))}
          </dl>
        </div>
      </Section>

      <CtaBand
        id="try-free"
        title="Try the Free plan now"
        lead={`${SITE.product} ${RELEASE.version} runs the Free plan on your computer, with no account, on Linux.`}
      >
        <a href="/download/" className={siteButton("primary", "lg")}>
          <Icon icon={Download} size={18} />
          Download for Linux
        </a>
        <a href="/docs/" className={siteButton("secondary", "lg")}>
          Read the docs
        </a>
      </CtaBand>
    </SiteFrame>
  );
}
