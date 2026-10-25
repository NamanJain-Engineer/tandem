import { EngineGlyph } from "@tandem/ui/components/EngineGlyph";
import registry from "../../../../../crates/tandem-engines/registry.json";
import { ENGINES } from "../../../lib/site";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";

/** An engine's registry entry by its label: its id (which picks the tile's
 * tint, D-1895) and its `logo` letters. */
export function engineMark(label: string): { id: string; logo: string } {
  const entry = (registry as Array<{ id: string; label: string; logo: string }>).find((e) => e.label === label);
  if (!entry) throw new Error(`${label} is not in crates/tandem-engines/registry.json`);
  return { id: entry.id, logo: entry.logo };
}

/** An engine's monogram, from the registry's `logo` letters. Never a vendor logo. */
export function engineLetters(label: string): string {
  return engineMark(label).logo;
}

/** #engines (§6.5): the 13 engines as chips, the grid at the section's full
 * width (D-1897, D-1885: the old launch-grid capture is gone). With 13, the
 * plain-terminal cell fills the last row: two columns of three, or the
 * fourteenth cell of seven. */
export function Engines() {
  return (
    <Section
      id="engines"
      eyebrow="Engines"
      align="split"
      title="Bring the agents you already use"
      lead="Vibezeo finds the agent CLIs on your machine and starts each one signed in to your own account. Missing ones come with install steps."
    >
      <div className="flex flex-col gap-5">
        <ul className="surface grid grid-cols-2 overflow-hidden p-0 sm:grid-cols-3 lg:grid-cols-7">
          {ENGINES.map((e, i) => (
            <li
              key={e}
              className={`reveal ${stagger(i)} flex min-h-20 items-center gap-3 border-b border-r border-line px-4 py-4 text-copy-sm font-medium text-fg [&:nth-child(2n)]:max-sm:border-r-0 sm:[&:nth-child(3n)]:border-r-0 lg:[&:nth-child(3n)]:border-r lg:[&:nth-child(7n)]:border-r-0 max-sm:[&:nth-last-child(-n+2)]:border-b-0 sm:[&:nth-last-child(-n+2)]:border-b-0 lg:[&:nth-last-child(-n+7)]:border-b-0`}
            >
              <EngineGlyph letters={engineLetters(e)} engine={engineMark(e).id} size={32} />
              {e}
            </li>
          ))}
          <li className="flex min-h-20 items-center px-4 py-4 text-caption text-fg-muted sm:col-span-2 lg:col-span-1">
            And a plain terminal.
          </li>
        </ul>
        <p className="text-caption text-fg-muted">
          Product names belong to their owners. Vibezeo isn&apos;t affiliated with or endorsed by any engine
          vendor.
        </p>
      </div>
    </Section>
  );
}
