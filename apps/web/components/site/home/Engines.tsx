import { EngineGlyph } from "@tandem/ui/components/EngineGlyph";
import { Pause, Play, SquareTerminal } from "lucide-react";
import registry from "../../../../../crates/tandem-engines/registry.json";
import { ENGINES } from "../../../lib/site";
import { Icon } from "../Icon";
import { Section } from "../Section";
import "./engines.css";

/** An engine's registry entry by its label: its id (which picks the tile's
 * tint, D-1895) and its `logo` letters. */
export function engineMark(label: string): { id: string; logo: string } {
  const entry = (registry as Array<{ id: string; label: string; logo: string }>).find(
    (e) => e.label === label,
  );
  if (!entry) throw new Error(`${label} is not in crates/tandem-engines/registry.json`);
  return { id: entry.id, logo: entry.logo };
}

/** An engine's monogram, from the registry's `logo` letters. Never a vendor logo. */
export function engineLetters(label: string): string {
  return engineMark(label).logo;
}

/** The two rows the engines drift in, and how many times each row's set is
 * drawn: enough that one set's width always covers the viewport while the
 * track moves by exactly one set (a seamless loop on screens up to 2560 px). */
const ROWS = [ENGINES.slice(0, 7), ENGINES.slice(7)] as const;
const COPIES = 4;

function Chip({ name, named }: { name: string; named: boolean }) {
  const mark = engineMark(name);
  return (
    <span className="engine-chip">
      <EngineGlyph letters={mark.logo} engine={mark.id} size={24} className="shrink-0" />
      {/* Only the first set carries the name hook the one-line check reads. */}
      <span
        data-engine-name={named ? "" : undefined}
        title={named ? name : undefined}
        className="engine-name"
      >
        {name}
      </span>
    </span>
  );
}

/** #engines (§6.5): the 13 engines drift past in two slow rows, in opposite
 * directions (DESIGN §4.7: the engines' slow motion), with the page's edges
 * fading them in and out. The rows are decoration for the list a screen
 * reader reads once; hovering a row, or Pause, holds them still, and under
 * reduced motion they are a still, wrapped grid. CSS only, no script. */
export function Engines() {
  return (
    <Section
      id="engines"
      eyebrow="Engines"
      align="split"
      className="overflow-x-clip"
      title="Bring the agents you already use"
      lead="Vibezeo finds the agent CLIs on your machine and starts each one signed in to your own account. Missing ones come with install steps."
    >
      <div className="flex flex-col gap-6">
        <ul className="sr-only">
          {ENGINES.map((e) => (
            <li key={e}>{e}</li>
          ))}
          <li>And a plain terminal.</li>
        </ul>
        <div aria-hidden="true" className="engine-marquee reveal">
          {ROWS.map((row, r) => (
            <div key={r} className="engine-row" data-dir={r === 0 ? "left" : "right"}>
              <div className="engine-track">
                {Array.from({ length: COPIES }, (_, copy) => (
                  <div key={copy} className="engine-set">
                    {row.map((e) => (
                      <Chip key={e} name={e} named={copy === 0} />
                    ))}
                    {r === 1 && (
                      <span className="engine-chip engine-chip-plain">
                        <span className="engine-plain-tile">
                          <Icon icon={SquareTerminal} size={14} />
                        </span>
                        <span className="engine-name">And a plain terminal</span>
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
          <p className="text-caption text-fg-muted">
            Product names belong to their owners. Vibezeo isn&apos;t affiliated with or endorsed by any engine
            vendor.
          </p>
          <label className="engine-pause">
            <input type="checkbox" className="sr-only" />
            <Icon icon={Pause} size={14} className="engine-pause-off" />
            <Icon icon={Play} size={14} className="engine-pause-on" />
            Pause
          </label>
        </div>
      </div>
    </Section>
  );
}
