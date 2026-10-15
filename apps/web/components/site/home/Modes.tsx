import { siteButton } from "@tandem/ui/components/SiteButton";
import { StatusDot, STATUS_LABEL, type Status } from "@tandem/ui/components/StatusDot";
import { ArrowUp, Check } from "lucide-react";
import { MODE_ROWS, feature } from "../../../lib/home";
import { Icon } from "../Icon";
import { Keys, type KeyContext } from "../Keys";
import { SCREENS } from "../../../lib/media";
import { ProductShot } from "../ProductShot";
import { Section } from "../Section";
import { TextLink } from "../TextLink";

type Row = (typeof MODE_ROWS)[number];
const row = (id: Row["id"]): Row => MODE_ROWS.find((r) => r.id === id) as Row;

/** Code mode's shortcuts, as Linux binds them (from the command registry). */
const CODE_KEYS: ReadonlyArray<{ id: string; context: KeyContext; label: string }> = [
  { id: "pane.split_right", context: "terminal", label: "Split right" },
  { id: "pane.split_down", context: "terminal", label: "Split down" },
  { id: "palette.open", context: "default", label: "Command palette" },
  { id: "mode.code", context: "default", label: "Code mode" },
];

function Facts({ facts }: { facts: readonly string[] }) {
  return (
    <ul className="flex flex-col gap-2.5">
      {facts.map((f) => (
        <li key={f} className="flex items-start gap-2.5 text-copy-sm text-fg">
          <Icon icon={Check} size={16} className="mt-1 shrink-0 text-accent" />
          {f}
        </li>
      ))}
    </ul>
  );
}

function RowText({ r, bodies }: { r: Row; bodies: readonly string[] }) {
  return (
    <div className="flex flex-col gap-5">
      <p className="eyebrow">{r.eyebrow}</p>
      <h3 className="text-display-md text-balance text-fg">{r.title}</h3>
      {bodies.map((b) => (
        <p key={b} className="text-copy text-pretty text-fg-muted">
          {b}
        </p>
      ))}
      <Facts facts={r.facts} />
      <TextLink href={r.docs} className="self-start">
        Read the docs
      </TextLink>
    </div>
  );
}

const bodies = (r: Row) => r.features.map((t) => feature(t).body);

/** #features (§6.7): the modes, each in its own shape. Real captures only;
 * where none exists yet, the row points at the live demo above. */
export function Modes() {
  const code = row("code");
  const thread = row("thread");
  const agent = row("agent");
  const dash = row("dashboard");
  return (
    <Section id="features" eyebrow="Working today" align="center" title="Built for running many agents at once">
      <div className="flex flex-col gap-24">
        <div className="flex flex-col gap-10">
          <ProductShot
            zoom
            shot={SCREENS.codeMode}
            caption="Four real terminals over Vibezeo's own repository: git history, htop, the Claude Code hook handler in vim, and a passing test run."
          />
          <div className="grid gap-10 lg:grid-cols-12 lg:gap-6">
            <div className="lg:col-span-6">
              <RowText r={code} bodies={bodies(code)} />
            </div>
            <dl className="grid content-start gap-x-6 gap-y-5 sm:grid-cols-2 lg:col-span-5 lg:col-start-8">
              {CODE_KEYS.map((k) => (
                <div key={k.id} className="reveal flex flex-col gap-2.5">
                  <dt className="text-caption text-fg-muted">
                    {k.label}
                    {k.context === "terminal" && <span className="text-fg-subtle"> · in a terminal</span>}
                  </dt>
                  <dd>
                    <Keys id={k.id} context={k.context} size="lg" />
                  </dd>
                </div>
              ))}
            </dl>
          </div>
        </div>

        <div className="grid items-center gap-10 lg:grid-cols-12 lg:gap-6">
          <div className="lg:col-span-5">
            <RowText r={thread} bodies={bodies(thread)} />
          </div>
          <div className="reveal lg:col-span-6 lg:col-start-7">
            <div className="surface flex flex-col gap-6 p-8">
              <p className="text-title text-fg">See Thread mode in the live demo</p>
              <p className="text-copy-sm text-fg-muted">
                The demo above opens on a thread with its first message ready. Its agent is simulated, and it
                shows what a real thread does:
              </p>
              <ol className="flex flex-col gap-4">
                {[
                  "Send the message that's waiting: the agent reads the project and edits two files.",
                  "It asks before running the tests. Answer the approval in the conversation.",
                  "Open Changes to see the diff of every file the thread touched.",
                ].map((s, i) => (
                  <li key={s} className="flex gap-4 text-copy-sm text-fg">
                    <span className="keycap !h-7 !min-w-7 shrink-0 !px-0 !text-[0.75rem] text-fg-muted">
                      {String(i + 1).padStart(2, "0")}
                    </span>
                    <span className="pt-0.5">{s}</span>
                  </li>
                ))}
              </ol>
              <a href="#try" className={siteButton("secondary", "md", "self-start")}>
                <Icon icon={ArrowUp} size={16} />
                Back to the demo
              </a>
            </div>
          </div>
        </div>

        <div className="grid gap-6 lg:grid-cols-2">
          <div className="reveal surface flex flex-col gap-8 p-8 lg:p-10">
            <RowText r={agent} bodies={bodies(agent)} />
          </div>
          <div className="reveal surface flex flex-col gap-8 p-8 lg:p-10">
            <RowText r={dash} bodies={bodies(dash)} />
            <StatusLegend />
          </div>
        </div>
      </div>
    </Section>
  );
}

/** The status shapes the app draws on every pane and in the Dashboard:
 * colour always comes with a shape (UI_SPEC §1.3). */
function StatusLegend() {
  const shown: Status[] = ["working", "waiting", "done", "idle"];
  return (
    <figure className="flex flex-col gap-3 border-t border-line pt-6">
      <ul className="grid grid-cols-2 gap-3">
        {shown.map((s) => (
          <li key={s} className="flex items-center gap-2.5 rounded-md px-3 py-2.5 text-copy-sm text-fg shadow-[inset_0_0_0_1px_var(--site-line-strong)]">
            <span aria-hidden="true" className="inline-flex">
              <StatusDot status={s} size={10} />
            </span>
            {STATUS_LABEL[s]}
          </li>
        ))}
      </ul>
      <figcaption className="text-caption text-fg-muted">
        The status shapes Vibezeo draws on every pane and in the Dashboard.
      </figcaption>
    </figure>
  );
}
