import { buttonClass } from "@tandem/ui/components/Button";
import { LIMITS } from "@tandem/protocol/plans";
import { Download, Info, ShieldCheck } from "lucide-react";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import registry from "../../../../crates/tandem-engines/registry.json";
import { CommandLine } from "../../components/site/CommandLine";
import { Icon } from "../../components/site/Icon";
import { Keys } from "../../components/site/Keys";
import { Section } from "../../components/site/Section";
import { SiteFrame } from "../../components/site/SiteFrame";
import { TextLink } from "../../components/site/TextLink";
import { RELEASES, versionPath } from "../../lib/changelog";
import { formatSize, releaseAssets } from "../../lib/manifest";
import { CHECKSUMS_URL, DOWNLOADS, RELEASE, assetUrl } from "../../lib/release";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Download", alternates: { canonical: "/download/" } };

/** Getting started, with the Linux shortcuts from the command registry (H5). */
const START: ReadonlyArray<{ title: string; body: ReactNode }> = [
  {
    title: "Have an agent signed in",
    body: "Have at least one coding agent CLI installed and signed in, the way you use it today: Claude Code, Codex, Gemini CLI or another. Vibezeo finds it and never touches its sign-in.",
  },
  {
    title: "Start on the Free plan",
    body: "Open the app and choose Start on the welcome screen. The Free plan runs on your computer, no account needed.",
  },
  {
    title: "Add a project folder",
    body: "The launch grid lists the agents it found: pick one to start it in a real terminal, or Terminal for a shell.",
  },
  {
    title: "Split, and keep an eye on them",
    body: (
      <>
        In a terminal, split right with <Keys id="pane.split_right" context="terminal" /> and down with{" "}
        <Keys id="pane.split_down" context="terminal" />. <Keys id="palette.open" /> opens the command palette.
        The Dashboard shows which agents are working and which are waiting for you.
      </>
    ),
  },
];

const engineList = (ids: readonly string[] | null) =>
  (ids ?? []).map((id) => registry.find((e) => e.id === id)?.label ?? id).join(" and ");

const INCLUDES = [
  `The Free plan: up to ${LIMITS.free.panes} panes and ${LIMITS.free.workspaces} workspaces, and threads with ${engineList(LIMITS.free.chatEngines)}.`,
  "New versions appear on this page.",
  "This build doesn't contact our servers and carries no analytics.",
];

/** /download/detect.js names the visitor's OS here (D-132); inner HTML, so
 * hydration leaves what the script writes alone (as D-128). Its row keeps
 * its height, so the note appearing moves nothing. */
const OS_NOTE = '<p data-os-note="" hidden class="text-copy-sm text-fg"></p>';

export default function DownloadPage() {
  const assets = releaseAssets();
  const older = RELEASES.filter((r) => r.version !== RELEASE.version);
  return (
    <SiteFrame current="/download/">
      <section aria-label="Introduction" className="relative isolate overflow-hidden bg-page">
        <div aria-hidden="true" className="bg-site-grid absolute inset-0 -z-10 [mask-image:linear-gradient(to_bottom,black_30%,transparent)]" />
        <div className="container-site flex flex-col items-center gap-5 pb-10 pt-16 text-center lg:pt-20">
          <img src="/icon.png" width={96} height={96} alt="" className="size-20 drop-shadow-[0_0_32px_var(--site-glow)] lg:size-24" />
          <h1 className="text-display-xl text-fg">Download {SITE.product}</h1>
          <p className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1 font-mono text-copy-sm text-fg-muted">
            <span>Released · {RELEASE.version}</span>
            <span aria-hidden="true">·</span>
            <a href={versionPath(RELEASE.version)} className="text-accent">
              Release notes
            </a>
          </p>
          <p className="max-w-measure text-lead text-pretty text-fg-muted">
            {SITE.product} {RELEASE.version} for Linux. The Free plan runs on your own computer, with the coding
            agents you already use.
          </p>
        </div>
      </section>

      <section aria-label="Downloads" className="bg-page pb-(--site-section-y)">
        <div className="container-site flex flex-col items-center gap-4">
          <div className="flex min-h-7 items-center justify-center gap-2">
            <script src="/download/detect.js" defer />
            <div dangerouslySetInnerHTML={{ __html: OS_NOTE }} />
          </div>
          <ul className="w-full max-w-narrow">
            {DOWNLOADS.filter((d) => d.available).map((d) => {
              const [primary, ...others] = d.assets;
              const files = assets.get(d.os) ?? [];
              const main = files[0];
              return (
                <li key={d.os} id={d.os} className="flex flex-col gap-8 rounded-lg border border-border bg-card p-6 sm:p-8">
                  <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between">
                    <div className="flex flex-col gap-1">
                      <h2 className="text-display-md text-fg">{d.name}</h2>
                      <p className="text-copy-sm text-fg-muted">{d.requirement}</p>
                    </div>
                    {primary && (
                      <a href={assetUrl(primary.file)} className={`${buttonClass("primary", "xl")} self-start sm:self-auto`}>
                        <Icon icon={Download} size={18} />
                        {primary.label}
                      </a>
                    )}
                  </div>
                  {others.map((a) => (
                    <a key={a.file} href={assetUrl(a.file)} className="text-copy-sm text-accent">
                      {a.label}
                    </a>
                  ))}
                  <dl className="grid gap-x-8 gap-y-4 border-t border-divider pt-6 text-copy-sm sm:grid-cols-[auto_1fr]">
                    {main && (
                      <>
                        <dt className="text-fg-muted">File</dt>
                        <dd className="font-mono text-fg">{main.file}</dd>
                        <dt className="text-fg-muted">Size</dt>
                        <dd className="font-mono text-fg">{formatSize(main.bytes)}</dd>
                      </>
                    )}
                    {files.map((a) => (
                      <div key={a.file} data-sha256={a.file} className="contents">
                        <dt className="text-fg-muted">SHA-256 of {a.file}</dt>
                        <dd className="select-all break-all font-mono text-fg">{a.sha256}</dd>
                      </div>
                    ))}
                  </dl>
                  <div className="flex flex-col gap-3">
                    <p className="text-copy-sm font-semibold text-fg">Install</p>
                    {d.steps.map((s) => {
                      const step = /^(.*?):\s*(sudo .+)$/.exec(s);
                      return step ? (
                        <div key={s} className="flex flex-col gap-1.5">
                          <p className="text-copy-sm text-fg-muted">{step[1]}</p>
                          <CommandLine command={step[2] ?? ""} />
                        </div>
                      ) : (
                        <p key={s} className="text-copy-sm text-fg-muted">
                          {s}
                        </p>
                      );
                    })}
                  </div>
                  <div className="flex flex-col gap-3 rounded-md border border-border bg-page p-4">
                    <p className="flex items-start gap-2.5 text-copy-sm text-fg-muted">
                      <Icon icon={ShieldCheck} size={18} className="mt-0.5 shrink-0 text-accent" />
                      <span>
                        The packages aren&apos;t code-signed, so the checksum is how you confirm they&apos;re ours:
                        compare the output of <code className="font-mono text-fg">sha256sum</code> with the value above,
                        or with{" "}
                        <a href={CHECKSUMS_URL} className="text-accent">
                          SHA256SUMS
                        </a>
                        .
                      </span>
                    </p>
                    {main && <CommandLine command={`sha256sum ${main.file}`} />}
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      </section>

      <Section id="getting-started" tone="raised" eyebrow="Getting started" title="Running in four steps">
        <ol className="grid gap-4 md:grid-cols-2">
          {START.map((s, i) => (
            <li key={s.title} className="flex gap-5 rounded-md border border-border bg-card p-6">
              <span className="inline-flex size-9 shrink-0 items-center justify-center rounded-sm border border-border-strong font-mono text-copy-sm text-fg-muted">
                {String(i + 1).padStart(2, "0")}
              </span>
              <div className="flex flex-col gap-2">
                <h3 className="text-title text-fg">{s.title}</h3>
                <p className="text-copy-sm leading-7 text-fg-muted">{s.body}</p>
              </div>
            </li>
          ))}
        </ol>
      </Section>

      <section aria-labelledby="includes-title" className="bg-page py-(--site-section-y)">
        <div className="container-site grid gap-10 lg:grid-cols-12 lg:gap-6">
          <div className="flex flex-col gap-4 lg:col-span-6">
            <h2 id="includes-title" className="text-display-md text-fg">
              What this release includes
            </h2>
            <ul className="flex flex-col gap-3">
              {INCLUDES.map((s) => (
                <li key={s} className="flex items-start gap-2.5 text-copy text-fg-muted">
                  <Icon icon={Info} size={16} className="mt-1.5 shrink-0 text-accent" />
                  {s}
                </li>
              ))}
            </ul>
            <p className="text-copy text-fg-muted">
              Found a problem? Write to <a href={`mailto:${SITE.contactEmail}`}>{SITE.contactEmail}</a>.
            </p>
          </div>
          {older.length > 0 && (
            <div className="flex flex-col gap-4 lg:col-span-5 lg:col-start-8">
              <h2 className="text-display-md text-fg">Previous versions</h2>
              <p className="text-copy-sm text-fg-muted">
                Only the current build is served. Earlier releases&apos; notes are in the changelog.
              </p>
              <ul className="flex flex-wrap gap-x-6 gap-y-1">
                {older.map((r) => (
                  <li key={r.version}>
                    <TextLink href={versionPath(r.version)} className="font-mono text-copy-sm">
                      {r.version}
                    </TextLink>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </section>
    </SiteFrame>
  );
}
