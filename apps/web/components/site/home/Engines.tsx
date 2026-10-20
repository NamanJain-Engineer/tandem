import { EngineGlyph } from "@tandem/ui/components/EngineGlyph";
import registry from "../../../../../crates/tandem-engines/registry.json";
import { ENGINES } from "../../../lib/site";
import { SCREENS } from "../../../lib/media";
import { ProductShot } from "../ProductShot";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";

/** An engine's monogram, from the registry's `logo` letters. Never a vendor logo. */
export function engineLetters(label: string): string {
  const entry = (registry as Array<{ label: string; logo: string }>).find((e) => e.label === label);
  if (!entry) throw new Error(`${label} is not in crates/tandem-engines/registry.json`);
  return entry.logo;
}

/** #engines (§6.5): the 13 engines as chips, and the real launch grid.
 * With 13, the plain-terminal cell spans two columns so the last row fills. */
export function Engines() {
  return (
    <Section
      id="engines"
      eyebrow="Engines"
      align="split"
      title="Bring the agents you already use"
      lead="Vibezeo finds the engine CLIs installed on your machine and launches them in panes, each signed in to your own account. If an engine is missing, Vibezeo shows you how to install it."
    >
      <div className="grid items-start gap-10 lg:grid-cols-12 lg:gap-8">
        <div className="flex flex-col gap-5 lg:col-span-7">
          <ul className="surface grid grid-cols-2 overflow-hidden p-0 sm:grid-cols-3">
            {ENGINES.map((e, i) => (
              <li
                key={e}
                className={`reveal ${stagger(i)} flex min-h-20 items-center gap-3 border-b border-r border-line px-4 py-4 text-copy-sm font-medium text-fg [&:nth-child(2n)]:max-sm:border-r-0 sm:[&:nth-child(3n)]:border-r-0 max-sm:[&:nth-last-child(-n+2)]:border-b-0 sm:[&:nth-last-child(-n+2)]:border-b-0`}
              >
                <EngineGlyph letters={engineLetters(e)} size={32} />
                {e}
              </li>
            ))}
            <li className="flex min-h-20 items-center px-4 py-4 text-caption text-fg-muted sm:col-span-2">
              And a plain terminal.
            </li>
          </ul>
          <p className="text-caption text-fg-muted">
            Product names belong to their owners. Vibezeo isn&apos;t affiliated with or endorsed by any engine
            vendor.
          </p>
        </div>
        <ProductShot
          className="lg:col-span-5"
          sizes="(min-width: 1024px) 480px, 100vw"
          shot={SCREENS.launchGrid}
          caption="A new tab: Vibezeo found Claude Code on this machine and offers to install the ten engines that aren't."
        />
      </div>
    </Section>
  );
}
