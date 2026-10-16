import { EngineGlyph } from "@tandem/ui/components/EngineGlyph";
import type { CSSProperties } from "react";
import { ENGINES } from "../../lib/site";
import { engineLetters } from "./home/Engines";

/** The engines Vibezeo runs, as their monograms on a slow orbit round the
 * hero's rings (D-1842): every vendor's agent, around one workspace. It's
 * an HTML layer turned with transforms only, so the compositor carries it;
 * each monogram turns back the other way to stay upright. Decorative (the
 * lead names the engines), hidden on phones and still under reduced motion.
 * `top` is the rings' centre, in px from the top of the hero. */
export function EngineOrbit({ top, radius }: { top: number; radius: number }) {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 -z-10 hidden overflow-hidden md:block [mask-image:linear-gradient(to_bottom,black_30%,transparent_80%)]"
    >
      <div className="orbit-spin absolute left-1/2 size-0" style={{ top }}>
        {ENGINES.map((e, i) => (
          <span
            key={e}
            className="absolute left-0 top-0"
            style={
              {
                transform: `rotate(${(360 / ENGINES.length) * i - 90}deg) translateX(${radius}px) rotate(${90 - (360 / ENGINES.length) * i}deg)`,
              } as CSSProperties
            }
          >
            <span className="orbit-upright -ml-[1.375rem] -mt-[1.375rem] flex size-11 items-center justify-center rounded-full bg-(--site-glass) shadow-[inset_0_1px_0_var(--site-edge-hi),inset_0_0_0_1px_var(--site-line-strong)]">
              <EngineGlyph letters={engineLetters(e)} size={32} />
            </span>
          </span>
        ))}
      </div>
    </div>
  );
}
