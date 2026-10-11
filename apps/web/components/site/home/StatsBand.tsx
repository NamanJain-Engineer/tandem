import { ArrowRight } from "lucide-react";
import { linuxFacts } from "../../../lib/home";
import { PROGRESS } from "../../../lib/progress";
import { RELEASE } from "../../../lib/release";
import { Icon } from "../Icon";

/** The proof band (§6.4): figures from PROGRESS, never typed in. */
export function StatsBand() {
  const stats = [
    { value: PROGRESS.milestones.toLocaleString("en-US"), label: `milestones in the ${RELEASE.version} release` },
    { value: PROGRESS.automatedTests.toLocaleString("en-US"), label: "automated tests in its code" },
    { value: PROGRESS.endToEndSuites.toLocaleString("en-US"), label: "end-to-end suites on the real app" },
    { value: PROGRESS.engines.toLocaleString("en-US"), label: "coding agents supported" },
  ];
  return (
    <section aria-label="Progress" className="border-y border-border bg-raised">
      <div className="container-site flex flex-col gap-6 py-12">
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-md bg-border lg:grid-cols-4">
          {stats.map((s) => (
            <div key={s.label} className="flex flex-col gap-2 bg-raised p-5 lg:p-6">
              <dt className="order-2 text-caption text-fg-muted">{s.label}</dt>
              <dd className="order-1 font-mono text-stat tabular-nums text-fg">{s.value}</dd>
            </div>
          ))}
        </dl>
        <p className="text-copy-sm text-fg">
          <span className="font-semibold">Linux first.</span>{" "}
          <span className="text-fg-muted">{linuxFacts().join(" · ")}.</span>
        </p>
        <p className="text-caption text-fg-muted">
          Built so far.{" "}
          <a href="/changelog/" className="inline-flex items-center gap-1 font-medium text-accent">
            See the changelog
            <Icon icon={ArrowRight} size={14} />
          </a>
        </p>
      </div>
    </section>
  );
}
