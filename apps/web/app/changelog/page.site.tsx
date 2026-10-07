import type { Metadata } from "next";
import { SiteFrame } from "../../components/site/SiteFrame";
import { MILESTONES, PROGRESS } from "../../lib/progress";
import { RELEASE, RELEASE_URL } from "../../lib/release";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Changelog" };

export default function ChangelogPage() {
  return (
    <SiteFrame>
      <section className="mx-auto flex max-w-3xl flex-col gap-4 px-6 pb-6 pt-16">
        <p className="text-xs font-medium uppercase tracking-caps text-accent">Changelog</p>
        <h1 className="text-3xl font-semibold tracking-tight">What's shipped in {SITE.product} so far</h1>
        <p className="text-lg text-fg-muted">
          {SITE.product} is built in milestones. Each one closes with its features tested end to end on the
          real app: {PROGRESS.automatedTests.toLocaleString("en-US")} automated tests and{" "}
          {PROGRESS.endToEndSuites} end-to-end suites as of {PROGRESS.asOf}. The first release,{" "}
          <a href={RELEASE_URL} className="text-accent hover:underline">
            {SITE.product} {RELEASE.version} for Linux
          </a>
          , came out in {RELEASE.date}:{" "}
          <a href="/download/" className="text-accent hover:underline">
            download it
          </a>
          .
        </p>
      </section>
      <ol className="mx-auto flex max-w-3xl flex-col px-6 pb-16">
        {MILESTONES.map((m) => (
          <li key={m.id} id={m.id.toLowerCase()} className="flex flex-col gap-3 border-t border-divider py-8">
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <span className="font-mono text-sm text-accent">{m.id}</span>
              <h2 className="text-xl font-semibold tracking-tight">{m.title}</h2>
              <span className="text-sm text-fg-subtle">{m.month}</span>
            </div>
            <ul className="flex flex-col gap-1.5 text-base text-fg-muted">
              {m.items.map((item) => (
                <li key={item} className="ml-5 list-disc">
                  {item}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ol>
    </SiteFrame>
  );
}
