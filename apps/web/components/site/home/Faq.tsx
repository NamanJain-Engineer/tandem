import { Plus } from "lucide-react";
import { FAQ } from "../../../lib/home";
import { SITE } from "../../../lib/site";
import { Icon } from "../Icon";

/** #faq (§6.20): native <details> in one exclusive group; no script. */
export function Faq() {
  return (
    <section id="faq" aria-labelledby="faq-title" className="relative scroll-mt-section bg-page py-(--site-section-y)">
      <div className="container-site grid gap-10 lg:grid-cols-12 lg:gap-6">
        <header className="reveal flex flex-col gap-4 lg:col-span-4">
          <p className="eyebrow">FAQ</p>
          <h2 id="faq-title" className="text-sheen text-display-lg text-fg">
            Questions
          </h2>
          <p className="text-copy text-fg-muted">
            Not answered here? Write to <a href={`mailto:${SITE.contactEmail}`} className="text-fg">{SITE.contactEmail}</a>.
          </p>
        </header>
        <div className="flex flex-col border-t border-line lg:col-span-8">
          {FAQ.map((f, i) => (
            <details key={f.q} name="faq" open={i === 0} className="group border-b border-line">
              <summary className="flex min-h-18 cursor-pointer list-none items-center justify-between gap-6 py-5 text-title text-fg transition-colors duration-(--dur-base) hover:text-fg-muted [&::-webkit-details-marker]:hidden">
                {f.q}
                <Icon
                  icon={Plus}
                  size={20}
                  className="shrink-0 text-fg-muted transition-transform duration-(--dur-panel) ease-(--ease-spring-site) group-open:rotate-45"
                />
              </summary>
              <p className="max-w-[44rem] pb-6 text-copy text-pretty text-fg-muted">{f.a}</p>
            </details>
          ))}
        </div>
      </div>
    </section>
  );
}
