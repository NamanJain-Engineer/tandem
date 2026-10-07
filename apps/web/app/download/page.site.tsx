import { Badge } from "@tandem/ui/components/Badge";
import type { Metadata } from "next";
import { SiteFrame } from "../../components/site/SiteFrame";
import { CHECKSUMS_URL, DOWNLOADS, RELEASE, RELEASE_URL, assetUrl } from "../../lib/release";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Download" };

const START = [
  "Have at least one coding agent CLI installed and signed in, the way you use it today: Claude Code, Codex, Gemini CLI or another. Vibezeo finds it and never touches its sign-in.",
  "Open the app and choose Start on the welcome screen. The Free plan runs on your computer, no account needed.",
  "Add a project folder. The launch grid lists the agents it found: pick one to start it in a real terminal, or Terminal for a shell.",
  "Split with Mod+D (right) and Mod+Shift+D (down), where Mod is ⌘ on a Mac and Ctrl elsewhere. Mod+K opens the command palette. The Dashboard shows which agents are working and which are waiting for you.",
];

const LIMITS = [
  "The Free plan: up to 4 panes and 3 workspaces.",
  "No accounts yet, so no Pro, Team or hosted AI features. They come when accounts open.",
  "No automatic updates: new builds appear on this page.",
  "This build doesn't contact our servers and carries no analytics.",
];

export default function DownloadPage() {
  return (
    <SiteFrame>
      <section className="mx-auto flex max-w-6xl flex-col items-start gap-4 px-6 pb-8 pt-16">
        <Badge variant="accent" dot>
          Released · {RELEASE.version}
        </Badge>
        <h1 className="text-3xl font-semibold tracking-tight">Download {SITE.product}</h1>
        <p className="max-w-3xl text-lg text-fg-muted">
          {SITE.product} {RELEASE.version} is released for Linux. macOS and Windows are next. The Free plan
          runs on your own computer, with the coding agents you already use.{" "}
          <a href={RELEASE_URL} className="text-accent hover:underline">
            Release notes
          </a>
        </p>
      </section>

      <section aria-label="Downloads" className="mx-auto max-w-6xl px-6 pb-12">
        <ul className="grid gap-4 md:grid-cols-3">
          {DOWNLOADS.map((d) => {
            const [primary, ...others] = d.assets;
            return (
              <li
                key={d.os}
                id={d.os}
                className="flex flex-col gap-4 rounded-md border border-border bg-surface-1 p-6"
              >
                <div className="flex flex-col gap-1">
                  <h2 className="flex items-center gap-2 text-lg font-semibold">
                    {d.name}
                    {!d.available && <Badge variant="outline">Coming soon</Badge>}
                  </h2>
                  <p className="text-sm text-fg-muted">{d.requirement}</p>
                </div>
                {!d.available && (
                  <p className="text-sm text-fg-muted">
                    The {d.name} build is next. Write to{" "}
                    <a href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
                      {SITE.contactEmail}
                    </a>{" "}
                    to hear when it&apos;s ready.
                  </p>
                )}
                {d.available && primary && (
                  <a
                    href={assetUrl(primary.file)}
                    className="self-start rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover"
                  >
                    {primary.label}
                  </a>
                )}
                {d.available &&
                  others.map((a) => (
                    <a key={a.file} href={assetUrl(a.file)} className="text-sm text-accent hover:underline">
                      {a.label}
                    </a>
                  ))}
                {d.available && (
                  <ol className="flex flex-col gap-1.5 border-t border-divider pt-4 text-sm text-fg-muted">
                    {d.steps.map((s) => (
                      <li key={s} className="ml-4 list-decimal">
                        {s}
                      </li>
                    ))}
                  </ol>
                )}
              </li>
            );
          })}
        </ul>
        <p className="mt-4 text-sm text-fg-muted">
          Check a download against{" "}
          <a href={CHECKSUMS_URL} className="text-accent hover:underline">
            SHA256SUMS
          </a>
          . The build isn't code-signed yet.
        </p>
      </section>

      <section id="getting-started" className="border-t border-divider">
        <div className="mx-auto grid max-w-6xl gap-10 px-6 py-12 md:grid-cols-2">
          <div className="flex flex-col gap-3">
            <h2 className="text-xl font-semibold tracking-tight">Getting started</h2>
            <ol className="flex flex-col gap-2 text-base text-fg-muted">
              {START.map((s) => (
                <li key={s} className="ml-5 list-decimal">
                  {s}
                </li>
              ))}
            </ol>
          </div>
          <div className="flex flex-col gap-3">
            <h2 className="text-xl font-semibold tracking-tight">What this release includes</h2>
            <ul className="flex flex-col gap-2 text-base text-fg-muted">
              {LIMITS.map((s) => (
                <li key={s} className="ml-5 list-disc">
                  {s}
                </li>
              ))}
            </ul>
            <p className="text-base text-fg-muted">
              Found a problem? Write to{" "}
              <a href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
                {SITE.contactEmail}
              </a>
              .
            </p>
          </div>
        </div>
      </section>
    </SiteFrame>
  );
}
