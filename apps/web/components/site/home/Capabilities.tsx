import { ArrowRight } from "lucide-react";
import { CAPABILITIES, MORE } from "../../../lib/home";
import { Icon } from "../Icon";
import { iconFor } from "../icons";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";

/** #capabilities (§6.9, D-1881, D-1885): the eight capabilities people use
 * most, one line each (the name, linking to its docs), then everything else
 * as a line of links; what each does is in the docs. */
export function Capabilities() {
  return (
    <Section id="capabilities" eyebrow="Everything in the app" title="Built in, working today" divider>
      <ul className="grid gap-x-10 gap-y-2 sm:grid-cols-2 lg:grid-cols-4">
        {CAPABILITIES.map((c, i) => (
          <li key={c.title} className={`reveal ${stagger(i)}`}>
            <a
              href={c.docs}
              className="group -mx-3 flex h-full items-center gap-4 rounded-[10px] p-3 transition-colors duration-(--dur-base) hover:bg-(--site-glass)"
            >
              <span className="inline-flex size-9 shrink-0 items-center justify-center rounded-[10px] text-fg-muted shadow-[inset_0_1px_0_var(--site-edge-hi),inset_0_0_0_1px_var(--site-line-strong)]">
                <Icon icon={iconFor(c.icon)} size={18} />
              </span>
              <span className="flex items-center gap-1.5 text-copy font-semibold text-fg">
                {c.title}
                <Icon
                  icon={ArrowRight}
                  size={14}
                  className="text-fg-subtle opacity-0 transition-opacity duration-(--dur-fast) group-hover:opacity-100 group-focus-visible:opacity-100"
                />
              </span>
            </a>
          </li>
        ))}
      </ul>
      <p className="reveal flex flex-wrap items-baseline gap-x-2 gap-y-1 text-copy-sm text-fg-muted">
        <a href="/docs/" className="font-medium text-fg underline decoration-[color-mix(in_oklab,currentColor_35%,transparent)] underline-offset-[0.2em]">
          Everything else in the docs
        </a>
        <span aria-hidden="true">·</span>
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
