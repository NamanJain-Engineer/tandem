import { Badge } from "@tandem/ui/components/Badge";
import type { Metadata } from "next";
import { SiteFrame } from "../../components/site/SiteFrame";
import { CREDIT_ROWS, CREDIT_VALUE_USD, PLANS } from "../../lib/pricing";
import { RELEASE } from "../../lib/release";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Pricing", alternates: { canonical: "/pricing/" } };

export default function PricingPage() {
  return (
    <SiteFrame>
      <section className="mx-auto flex max-w-6xl flex-col items-start gap-4 px-6 pb-8 pt-16">
        <Badge variant="accent" dot>
          Plans
        </Badge>
        <h1 className="text-3xl font-semibold tracking-tight">
          Free to run your agents. Pro when you run a lot.
        </h1>
        <p className="max-w-3xl text-lg text-fg-muted">
          Free is available now: download {SITE.product} {RELEASE.version} for Linux. Pro and Team start with
          Vibezeo accounts. Your coding agents always run on your own subscriptions, so {SITE.product} never
          charges for engine usage.
        </p>
      </section>

      <section aria-label="Plans" className="mx-auto max-w-6xl px-6 pb-12">
        <ul className="grid gap-4 md:grid-cols-3">
          {PLANS.map((plan) => (
            <li
              key={plan.id}
              id={plan.id}
              className={`flex flex-col gap-4 rounded-md border bg-surface-1 p-6 ${
                plan.id === "pro" ? "border-accent" : "border-border"
              }`}
            >
              <div className="flex flex-col gap-1">
                <h2 className="text-lg font-semibold">{plan.name}</h2>
                <p className="text-base text-fg-muted">{plan.summary}</p>
              </div>
              <p className="flex flex-wrap items-baseline gap-x-2">
                <span className="text-3xl font-semibold tracking-tight">{plan.price}</span>
                <span className="text-sm text-fg-muted">{plan.cadence}</span>
              </p>
              {plan.trial && <p className="text-sm text-fg-muted">{plan.trial}</p>}
              <ul className="flex flex-col gap-2 border-t border-divider pt-4 text-base">
                {plan.features.map((f) => (
                  <li key={f.text} className="flex items-start gap-2">
                    <span aria-hidden="true" className="text-accent">
                      ✓
                    </span>
                    <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
                      <span className={f.planned ? "text-fg-muted" : "text-fg"}>{f.text}</span>
                      {f.planned && <Badge variant="outline">Planned</Badge>}
                    </span>
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      </section>

      <section className="border-t border-divider">
        <div className="mx-auto grid max-w-6xl gap-8 px-6 py-12 md:grid-cols-2">
          <div className="flex flex-col gap-2">
            <h2 className="text-xl font-semibold tracking-tight">What credits pay for</h2>
            <p className="text-md text-fg-muted">
              Credits pay only for hosted AI features, which run on the Claude API: session titles, and
              Enhance and Polish for your prompts. One credit is ${CREDIT_VALUE_USD.toFixed(3)} of usage. They
              never pay for your engines.
            </p>
            <table className="mt-2 text-left text-sm">
              <caption className="sr-only">Credits per hosted feature</caption>
              <tbody>
                {CREDIT_ROWS.map((row) => (
                  <tr key={row.key} data-credit-price={row.key} className="border-t border-divider">
                    <th scope="row" className="py-1.5 pr-6 font-medium text-fg">
                      {row.label}
                    </th>
                    <td className="py-1.5 text-fg-muted">
                      {row.credits === 0 ? "Free" : `${row.credits} credits ${row.unit}`}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="flex flex-col items-start gap-3">
            <h2 className="text-xl font-semibold tracking-tight">Try the Free plan now</h2>
            <p className="text-md text-fg-muted">
              {SITE.product} {RELEASE.version} runs the Free plan on your computer, with no account, on Linux.
            </p>
            <a
              href="/download/"
              className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover"
            >
              Download →
            </a>
          </div>
        </div>
      </section>
    </SiteFrame>
  );
}
