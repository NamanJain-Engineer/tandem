import { ArrowRight } from "lucide-react";
import { CAPABILITIES, MORE, feature, firstSentence } from "../../../lib/home";
import { Icon } from "../Icon";
import { iconFor } from "../icons";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";

/** The sentence that states a feature's plan limit ("Free includes one
 * connection."), so the short line keeps it (plan limits stated where they
 * apply, OPERATING §9). */
function planLimit(body: string): string | undefined {
  return body.split(/(?<=\.) /).find((x) => /^Free /.test(x));
}

/** #capabilities (§6.9, D-1881): the eight capabilities people use most,
 * one line each, then everything else as a line of links; the full copy is
 * in the docs. */
export function Capabilities() {
  return (
    <Section id="capabilities" eyebrow="Everything in the app" title="And the rest of the workspace" divider>
      <ul className="grid gap-x-10 gap-y-2 sm:grid-cols-2 lg:grid-cols-4">
        {CAPABILITIES.map((c, i) => (
          <li key={c.title} className={`reveal ${stagger(i)}`}>
            <a
              href={c.docs}
              className="group -mx-3 flex h-full gap-4 rounded-[10px] p-3 transition-colors duration-(--dur-base) hover:bg-(--site-glass)"
            >
              <span className="mt-0.5 inline-flex size-9 shrink-0 items-center justify-center rounded-[10px] text-fg-muted shadow-[inset_0_1px_0_var(--site-edge-hi),inset_0_0_0_1px_var(--site-line-strong)]">
                <Icon icon={iconFor(c.icon)} size={18} />
              </span>
              <span className="flex flex-col gap-1">
                <span className="flex items-center gap-1.5 text-copy font-semibold text-fg">
                  {c.title}
                  <Icon
                    icon={ArrowRight}
                    size={14}
                    className="text-fg-subtle opacity-0 transition-opacity duration-(--dur-fast) group-hover:opacity-100 group-focus-visible:opacity-100"
                  />
                </span>
                <span className="text-copy-sm text-fg-muted">
                  {firstSentence(feature(c.title).body)}
                  {planLimit(feature(c.title).body) && <span className="text-fg-subtle"> {planLimit(feature(c.title).body)}</span>}
                </span>
              </span>
            </a>
          </li>
        ))}
      </ul>
      <p className="reveal flex flex-wrap items-baseline gap-x-2 gap-y-1 text-copy-sm text-fg-muted">
        <span className="font-medium text-fg">Everything else:</span>
        {MORE.map((m, i) => (
          <span key={m.title}>
            <a href={m.docs} className="text-fg-muted underline decoration-[color-mix(in_oklab,currentColor_35%,transparent)] underline-offset-[0.2em] hover:text-fg">
              {m.title}
            </a>
            {i < MORE.length - 1 ? "," : "."}
          </span>
        ))}
      </p>
    </Section>
  );
}
