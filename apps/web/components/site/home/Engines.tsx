import { EngineGlyph } from "@tandem/ui/components/EngineGlyph";
import registry from "../../../../../crates/tandem-engines/registry.json";
import { ENGINES } from "../../../lib/site";
import { ProductShot } from "../ProductShot";
import { Section } from "../Section";

/** An engine's monogram, from the registry's `logo` letters. Never a vendor logo. */
export function engineLetters(label: string): string {
  const entry = (registry as Array<{ label: string; logo: string }>).find((e) => e.label === label);
  if (!entry) throw new Error(`${label} is not in crates/tandem-engines/registry.json`);
  return entry.logo;
}

/** #engines (§6.5): the 11 engines as chips, and the real launch grid. */
export function Engines() {
  return (
    <Section
      id="engines"
      eyebrow="Engines"
      title="Bring the agents you already use"
      lead="Vibezeo finds the engine CLIs installed on your machine and launches them in panes, each signed in to your own account. If an engine is missing, Vibezeo shows you how to install it."
    >
      <div className="grid items-center gap-10 lg:grid-cols-12 lg:gap-6">
        <div className="flex flex-col gap-6 lg:col-span-7">
          <ul className="flex flex-wrap gap-2">
            {ENGINES.map((e) => (
              <li
                key={e}
                className="inline-flex h-11 items-center gap-2.5 rounded-full border border-border bg-card pl-1.5 pr-4 text-copy-sm text-fg"
              >
                <EngineGlyph letters={engineLetters(e)} size={32} />
                {e}
              </li>
            ))}
          </ul>
          <p className="text-caption text-fg-muted">
            Product names belong to their owners. Vibezeo isn&apos;t affiliated with or endorsed by any engine
            vendor.
          </p>
        </div>
        <ProductShot
          className="lg:col-span-5"
          sizes="(min-width: 1024px) 480px, 100vw"
          shot={{
            src: "/screens/launch-grid.png",
            width: 544,
            height: 570,
            formats: ["avif", "webp"],
            alt: "Vibezeo's launch grid in a new tab: Claude Code 2.1.292 detected, a terminal, and ten more engines with Install buttons.",
          }}
          caption="A new tab: Vibezeo found Claude Code on this machine and offers to install the ten engines that aren't."
        />
      </div>
    </Section>
  );
}
