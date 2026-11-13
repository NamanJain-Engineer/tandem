import { ChevronDown } from "lucide-react";
import { FAQ, HOME_FAQ } from "../../../lib/home";
import { SITE } from "../../../lib/site";
import { Icon } from "../Icon";
import "./faq.css";

/** #faq (§6.20): native <details> in one exclusive group; no script. The
 * home page asks four (HOME_FAQ, DESIGN §5.1). Opening one turns its chevron
 * and lifts the answer in (transform and opacity, off under reduced motion). */
export function Faq() {
  const items = HOME_FAQ.map((q) => FAQ.find((f) => f.q === q)).filter((f) => f !== undefined);
  return (
    <section
      id="faq"
      aria-labelledby="faq-title"
      className="relative scroll-mt-section bg-page py-(--site-section-y)"
    >
      <div className="container-site grid gap-10 lg:grid-cols-12 lg:gap-6">
        <header className="reveal flex flex-col gap-4 lg:col-span-4">
          <p className="eyebrow">FAQ</p>
          <h2 id="faq-title" className="text-sheen text-display-lg text-fg">
            Questions
          </h2>
          <p className="max-w-[22rem] text-copy text-pretty text-fg-muted">
            Not answered here? Write to{" "}
            <a href={`mailto:${SITE.contactEmail}`} className="text-fg">
              {SITE.contactEmail}
            </a>
            .
          </p>
        </header>
        <div className="faq-list reveal stagger-1 lg:col-span-7 lg:col-start-6">
          {items.map((f, i) => (
            <details key={f.q} name="faq" open={i === 0} className="faq-item">
              <summary className="faq-q">
                <span aria-hidden="true" className="faq-n">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <span className="faq-text">{f.q}</span>
                <span aria-hidden="true" className="faq-chev">
                  <Icon icon={ChevronDown} size={16} />
                </span>
              </summary>
              <p className="faq-a">{f.a}</p>
            </details>
          ))}
        </div>
      </div>
    </section>
  );
}
