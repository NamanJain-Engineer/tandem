import { ArrowLeftRight, HardDrive, KeyRound, Plug, Server, Terminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { RELEASE } from "../../../lib/release";
import { docsPath } from "../../../lib/docs/pages";
import { Icon } from "../Icon";
import { LitGlow } from "../LitGlow";
import { TextLink } from "../TextLink";
import "./trust.css";

type Node = { icon: LucideIcon; title: string; body: string };

const ON_COMPUTER: Node[] = [
  {
    icon: HardDrive,
    title: "Vibezeo",
    body: "Workspaces, layouts, history, notes and tasks, stored on your computer.",
  },
  { icon: KeyRound, title: "OS keychain", body: "Secrets and plugin credentials." },
  { icon: Terminal, title: "Engine CLIs", body: "Each signed in by itself, in its own folder." },
];

const FLOWS: Array<{
  icon: LucideIcon;
  to: string;
  via: string;
  line: "solid" | "none" | "dashed";
  from: string;
}> = [
  {
    icon: ArrowLeftRight,
    to: "Engine vendors",
    from: "Engine CLIs",
    via: "Directly, under their terms",
    line: "solid",
  },
  { icon: Server, to: "Vibezeo servers", from: "Vibezeo", via: "Nothing in this build", line: "none" },
  {
    icon: Plug,
    to: "Services you connect",
    from: "Plugins",
    via: "Only what you allow; writes ask you first",
    line: "dashed",
  },
];

/** #principles (§6.10): how data moves. The four principles that followed
 * repeated the ownership chips and live on /about/ (D-1897, D-1885). The
 * diagram is HTML, so it reflows on a phone; its text is the description.
 * On a wide screen each route is a wire from your computer to where data
 * can go: drawn in once as the diagram scrolls in (a stroke dash on a
 * scroll timeline, trust.css), with packets only where data really moves
 * (D-1842). The nodes take the pointer's light (D-2576). */
export function TrustFlow() {
  return (
    <section
      id="principles"
      aria-labelledby="principles-title"
      className="relative isolate scroll-mt-section overflow-clip bg-raised py-(--site-section-y)"
    >
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-[30rem] opacity-70" />
      <div className="container-site flex flex-col gap-14">
        <header className="reveal flex flex-col gap-4">
          <p className="eyebrow">Ownership</p>
          <h2 id="principles-title" className="max-w-[28em] text-display-md text-fg [text-wrap:pretty]">
            Nothing leaves your machine unless you opt in.{" "}
            <span className="text-fg-muted">
              Engines sign in by themselves, in their own folders. Vibezeo never reads, copies or proxies
              their tokens.
            </span>
          </h2>
        </header>

        <figure className="trust flex flex-col gap-4">
          <div className="trust-map">
            <div data-lit="" className="trust-computer surface flex flex-col gap-3 p-5">
              <LitGlow />
              <p className="font-mono text-eyebrow uppercase tracking-[0.12em] text-fg-muted">
                Your computer
              </p>
              <ul className="flex flex-col gap-3">
                {ON_COMPUTER.map((n) => (
                  <li key={n.title} className="trust-node flex gap-3 p-4">
                    <Icon icon={n.icon} size={20} className="mt-0.5 shrink-0 text-fg-muted" />
                    <span className="flex flex-col gap-0.5">
                      <span className="text-copy font-semibold text-fg">{n.title}</span>
                      <span className="text-copy-sm text-fg-muted">{n.body}</span>
                    </span>
                  </li>
                ))}
              </ul>
            </div>
            <ul className="trust-routes">
              {FLOWS.map((f, i) => (
                <li key={f.to} className="trust-route" data-line={f.line}>
                  <div className="trust-wire">
                    <span className="trust-via text-caption text-fg-muted">
                      <span className="lg:hidden">{f.from}: </span>
                      {f.via}
                    </span>
                    <svg aria-hidden="true" focusable="false" className="trust-line" width="100%" height="12">
                      <mask
                        id={`trust-draw-${i}`}
                        className="trust-mask"
                        maskUnits="userSpaceOnUse"
                        x="0"
                        y="0"
                        width="100%"
                        height="100%"
                      >
                        <line className="trust-draw" x1="0" y1="6" x2="100%" y2="6" pathLength="1" />
                      </mask>
                      <line
                        className="trust-stroke"
                        x1="0"
                        y1="6"
                        x2="100%"
                        y2="6"
                        mask={`url(#trust-draw-${i})`}
                      />
                      <circle className="trust-port" cx="0" cy="6" r="3.5" />
                    </svg>
                    {/* D-1842: data moving where it really moves; nothing on the line to us. */}
                    {f.line !== "none" && (
                      <span aria-hidden="true" className="trust-packets">
                        <span className="flow-packet trust-packet">
                          <span className="trust-dot" />
                        </span>
                        {f.line === "solid" && (
                          <span className="flow-packet flow-back trust-packet">
                            <span className="trust-dot trust-dot-back" />
                          </span>
                        )}
                      </span>
                    )}
                  </div>
                  {f.line === "none" ? (
                    <div className="trust-dest trust-dest-none flex items-center gap-3 px-4 py-3 text-fg-muted">
                      <Icon icon={f.icon} size={18} className="text-fg-subtle" />
                      <span className="text-copy-sm font-medium line-through decoration-fg-subtle">
                        {f.to}
                      </span>
                    </div>
                  ) : (
                    <div data-lit="" className="trust-dest surface flex items-center gap-3 px-4 py-3 text-fg">
                      <LitGlow />
                      <Icon icon={f.icon} size={18} className="text-fg-muted" />
                      <span className="text-copy-sm font-medium">{f.to}</span>
                    </div>
                  )}
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
      </div>
    </section>
  );
}
