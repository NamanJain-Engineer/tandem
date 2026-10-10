import { ArrowRight } from "lucide-react";
import { CAPABILITIES, feature, firstSentence } from "../../../lib/home";
import { Icon } from "../Icon";
import { iconFor } from "../icons";
import { Section } from "../Section";

/** #capabilities (§6.9): everything else that works today, one line each;
 * the full copy is in the docs. */
export function Capabilities() {
  return (
    <Section id="capabilities" eyebrow="Everything in the app" title="And everything else that works today">
      <ul className="grid gap-x-10 gap-y-2 sm:grid-cols-2 lg:grid-cols-3">
        {CAPABILITIES.map((c) => (
          <li key={c.title} className="reveal">
            <a
              href={c.docs}
              className="group -mx-3 flex h-full gap-4 rounded-md p-3 transition-colors duration-(--dur-fast) hover:bg-card"
            >
              <span className="mt-0.5 inline-flex size-9 shrink-0 items-center justify-center rounded-md border border-border text-accent">
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
                <span className="text-copy-sm text-fg-muted">{firstSentence(feature(c.title).body)}</span>
              </span>
            </a>
          </li>
        ))}
      </ul>
    </Section>
  );
}
