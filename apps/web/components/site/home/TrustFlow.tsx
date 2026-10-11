import { ArrowLeftRight, ArrowRight, Ban, HardDrive, KeyRound, Plug, Server, Terminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { PRINCIPLES } from "../../../lib/home";
import { RELEASE } from "../../../lib/release";
import { cx } from "../../../lib/cx";
import { docsPath } from "../../../lib/docs/pages";
import { Icon } from "../Icon";
import { TextLink } from "../TextLink";

type Node = { icon: LucideIcon; title: string; body: string };

const ON_COMPUTER: Node[] = [
  { icon: HardDrive, title: "Vibezeo", body: "Workspaces, layouts, history, notes and tasks, stored on your computer." },
  { icon: KeyRound, title: "OS keychain", body: "Secrets and plugin credentials." },
  { icon: Terminal, title: "Engine CLIs", body: "Each signed in by itself, in its own folder." },
];

const FLOWS: Array<{ icon: LucideIcon; to: string; via: string; line: "solid" | "none" | "dashed"; from: string }> = [
  { icon: ArrowLeftRight, to: "Engine vendors", from: "Engine CLIs", via: "Directly, under their terms", line: "solid" },
  { icon: Ban, to: "Vibezeo servers", from: "Vibezeo", via: "Nothing in this build", line: "none" },
  { icon: Plug, to: "Services you connect", from: "Plugins", via: "Only what you allow; writes ask you first", line: "dashed" },
];

/** #principles (§6.10): how data moves, then the four principles. The
 * diagram is HTML, so it reflows on a phone; its text is the description. */
export function TrustFlow() {
  return (
    <section id="principles" aria-labelledby="principles-title" className="scroll-mt-(--site-header-h) bg-raised py-(--site-section-y)">
      <div className="container-site flex flex-col gap-14">
        <header className="reveal flex flex-col gap-4">
          <p className="eyebrow">Principles</p>
          <h2 id="principles-title" className="max-w-[30em] text-display-md text-balance text-fg">
            Nothing leaves your machine unless you opt in.{" "}
            <span className="text-fg-muted">
              Engines sign in by themselves, in their own folders, and Vibezeo never reads, copies or proxies their
              tokens.
            </span>
          </h2>
        </header>

        <figure className="reveal flex flex-col gap-4">
          <div className="grid gap-4 lg:grid-cols-12 lg:items-stretch lg:gap-0">
            <div className="flex flex-col gap-3 rounded-lg border border-border-strong bg-card p-5 lg:col-span-5">
              <p className="font-mono text-eyebrow uppercase tracking-[0.08em] text-fg-muted">Your computer</p>
              <ul className="flex flex-col gap-3">
                {ON_COMPUTER.map((n) => (
                  <li key={n.title} className="flex gap-3 rounded-md border border-border bg-page p-4">
                    <Icon icon={n.icon} size={20} className="mt-0.5 shrink-0 text-accent" />
                    <span className="flex flex-col gap-0.5">
                      <span className="text-copy font-semibold text-fg">{n.title}</span>
                      <span className="text-copy-sm text-fg-muted">{n.body}</span>
                    </span>
                  </li>
                ))}
              </ul>
            </div>
            <ul className="flex flex-col justify-around gap-3 lg:col-span-7">
              {FLOWS.map((f) => (
                <li key={f.to} className="grid items-center gap-2 lg:grid-cols-[1fr_auto] lg:gap-0">
                  <div className="flex items-center gap-3 lg:px-4">
                    <span
                      aria-hidden="true"
                      className={cx(
                        "hidden h-0 flex-1 border-t-2 lg:block",
                        f.line === "solid" && "border-accent",
                        f.line === "dashed" && "border-dashed border-fg-subtle",
                        f.line === "none" && "border-dotted border-border-strong",
                      )}
                    />
                    <span className="text-caption text-fg-muted">
                      <span className="lg:hidden">{f.from}: </span>
                      {f.via}
                    </span>
                    <Icon icon={ArrowRight} size={14} className="hidden text-fg-subtle lg:block" />
                  </div>
                  <div
                    className={cx(
                      "flex items-center gap-3 rounded-md border px-4 py-3 lg:w-60",
                      f.line === "none" ? "border-dashed border-border text-fg-muted" : "border-border bg-card text-fg",
                    )}
                  >
                    <Icon icon={f.line === "none" ? Server : f.icon} size={18} className={f.line === "none" ? "text-fg-subtle" : "text-accent"} />
                    <span className={cx("text-copy-sm font-medium", f.line === "none" && "line-through decoration-fg-subtle")}>
                      {f.to}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          </div>
          <figcaption className="flex flex-wrap items-center gap-x-6 gap-y-2 text-caption text-fg-muted">
            <span>Diagram: how data moves in Vibezeo {RELEASE.version}.</span>
            <TextLink href="/privacy/">Privacy policy</TextLink>
            <TextLink href={docsPath("privacy")}>Privacy and data flows</TextLink>
          </figcaption>
        </figure>

        <ul className="grid gap-x-10 gap-y-8 sm:grid-cols-2 lg:grid-cols-4">
          {PRINCIPLES.map((p) => (
            <li key={p.title} className="reveal flex flex-col gap-2 border-l-2 border-accent pl-4">
              <h3 className="text-title text-fg">{p.title}</h3>
              <p className="text-copy-sm text-fg-muted">{p.body}</p>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
