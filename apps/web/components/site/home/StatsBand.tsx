import { ArrowRight } from "lucide-react";
import { linuxFacts } from "../../../lib/home";
import { PROGRESS } from "../../../lib/progress";
import { RELEASE } from "../../../lib/release";
import { Icon } from "../Icon";
import { stagger } from "../../../lib/motion";

/** The proof band (§6.4): figures from PROGRESS, never typed in. */
export function StatsBand() {
  const stats = [
    { value: PROGRESS.milestones.toLocaleString("en-US"), label: `milestones in the ${RELEASE.version} release` },
    { value: PROGRESS.automatedTests.toLocaleString("en-US"), label: "automated tests in its code" },
    { value: PROGRESS.endToEndSuites.toLocaleString("en-US"), label: "end-to-end suites on the real app" },
    { value: PROGRESS.engines.toLocaleString("en-US"), label: "coding agents supported" },
  ];
  return (
    <section aria-label="Progress" className="relative bg-page">
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div aria-hidden="true" className="horizon absolute inset-x-0 bottom-0" />
      <div className="container-site flex flex-col gap-8 py-14 lg:py-16">
        <dl className="grid grid-cols-2 gap-y-10 lg:grid-cols-4">
          {stats.map((s, i) => (
            <div
              key={s.label}
              className={`reveal ${stagger(i)} flex flex-col gap-3 px-1 lg:px-8 ${i % 2 === 1 ? "border-l border-line pl-6" : ""} ${i > 0 ? "lg:border-l lg:border-line" : "lg:pl-0"}`}
            >
              <dt className="order-2 max-w-[14rem] text-caption text-fg-muted">{s.label}</dt>
              <dd className="text-sheen order-1 font-mono text-stat font-medium tabular-nums text-fg">{s.value}</dd>
            </div>
          ))}
        </dl>
        <div className="flex flex-col gap-2 border-t border-line pt-6 text-caption sm:flex-row sm:items-center sm:justify-between">
          <p className="text-fg">
            <span className="font-semibold">Linux first.</span>{" "}
            <span className="text-fg-muted">{linuxFacts().join(" · ")}.</span>
          </p>
          <p className="text-fg-muted">
            Built so far.{" "}
            <a href="/changelog/" className="link-arrow inline-flex items-center gap-1 font-medium text-fg no-underline">
              See the changelog
              <Icon icon={ArrowRight} size={14} />
            </a>
          </p>
        </div>
      </div>
    </section>
  );
}
