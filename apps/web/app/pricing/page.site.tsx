import { buttonClass } from "@tandem/ui/components/Button";
import { PRICES } from "@tandem/protocol/plans";
import { Check, Download } from "lucide-react";
import type { Metadata } from "next";
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

      <section aria-label="Plans" className="bg-page pb-(--site-section-y)">
        <ul className="container-site grid gap-4 md:grid-cols-3 md:gap-0">
          {PLANS.map((plan, i) => (
            <li
              key={plan.id}
              id={plan.id}
              className={cx(
                "flex scroll-mt-(--site-header-h) flex-col gap-6 border border-border bg-card p-6 lg:p-8",
                "max-md:rounded-lg",
                i === 0 && "md:rounded-l-lg",
                i === PLANS.length - 1 && "md:rounded-r-lg",
                i > 0 && "md:border-l-0",
                plan.id === "pro" && "relative md:z-10 md:outline md:outline-1 md:outline-accent",
              )}
            >
              <div className="flex flex-col gap-2">
                <h2 className="text-title text-fg">{plan.name}</h2>
                <p className="text-copy-sm text-fg-muted">{plan.summary}</p>
              </div>
              <p className="flex flex-col gap-1">
                <span className="font-mono text-stat tabular-nums text-fg">{plan.price}</span>
                <span className="text-caption text-fg-muted">{plan.cadence}</span>
              </p>
              {plan.id === "free" ? (
                <div className="flex flex-col gap-2">
                  <a href="/download/" className={`${buttonClass("primary", "lg")} self-start`}>
                    <Icon icon={Download} size={16} />
                    Download {RELEASE.version}
                  </a>
                  <p className="text-caption text-fg-muted">Available now, for Linux.</p>
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  <a href={notify(plan.name)} className={`${buttonClass("outline", "lg")} self-start`}>
                    Tell me when
                  </a>
                  <p className="text-caption text-fg-muted">
                    Starts with {SITE.product} accounts.{plan.trial ? ` ${plan.trial}.` : ""}
                  </p>
                </div>
              )}
              <ul className="flex flex-col gap-3 border-t border-divider pt-6 text-copy-sm">
                {plan.features.map((f) => (
                  <li key={f.text} className="flex items-start gap-2.5">
                    <Icon icon={Check} size={16} className="mt-1 shrink-0 text-accent" />
                    <span className={f.planned ? "text-fg-muted" : "text-fg"}>{f.text}</span>
                  </li>
                ))}
              </ul>
            </li>
          ))}
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
          <table className="w-full text-left text-copy-sm lg:col-span-6">
            <caption className="sr-only">Credits per hosted feature</caption>
            <tbody>
              {CREDIT_ROWS.map((row) => (
                <tr key={row.key} data-credit-price={row.key} className="border-b border-border">
                  <th scope="row" className="py-4 pr-6 font-medium text-fg">
                    {row.label}
                  </th>
                  <td className="py-4 text-right font-mono tabular-nums text-fg-muted">
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

      <section aria-labelledby="try-free-title" className="border-t border-border bg-raised">
        <div className="container-site flex flex-col items-start gap-5 py-16 md:flex-row md:items-center md:justify-between">
          <div className="flex flex-col gap-2">
            <h2 id="try-free-title" className="text-display-md text-fg">
              Try the Free plan now
            </h2>
            <p className="text-copy text-fg-muted">
              {SITE.product} {RELEASE.version} runs the Free plan on your computer, with no account, on Linux.
            </p>
          </div>
          <a href="/download/" className={buttonClass("primary", "xl")}>
            <Icon icon={Download} size={18} />
            Download for Linux
          </a>
        </div>
      </section>
    </SiteFrame>
  );
}
