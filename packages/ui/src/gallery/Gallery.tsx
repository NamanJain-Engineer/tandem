import { useState } from "react";
import { SegmentedControl } from "../components/SegmentedControl";
import { UiProvider, type ThemeSetting } from "../theme/ThemeProvider";
import { galleryEntries } from "./entries";

/** `/__gallery` (debug builds): every component state in either theme. */
export function Gallery() {
  const [theme, setTheme] = useState<ThemeSetting>("dark");
  return (
    <UiProvider theme={theme}>
      <main className="h-full overflow-y-auto bg-bg p-6 text-fg">
        <header className="mb-6 flex items-center gap-4">
          <h1 className="text-xl font-semibold">Component gallery</h1>
          <SegmentedControl label="Theme" value={theme} onValueChange={(v) => setTheme(v as ThemeSetting)} items={[{ id: "dark", label: "Dark" }, { id: "light", label: "Light" }]} />
        </header>
        <div className="flex flex-col gap-8">
          {galleryEntries.map((entry) => (
            <section key={entry.id} aria-labelledby={`g-${entry.id}`} className="flex flex-col gap-3">
              <h2 id={`g-${entry.id}`} className="text-lg font-semibold">
                {entry.title} <span className="text-sm font-normal text-fg-subtle">UI_SPEC {entry.spec}</span>
              </h2>
              <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-4">
                {entry.states
                  .filter((s) => !["dialog", "confirm", "confirm danger typed", "open"].includes(s.name) || entry.id === "tooltip")
                  .map((s) => (
                    <div key={s.name} className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3">
                      <span className="text-2xs uppercase tracking-caps text-fg-subtle">{s.name}</span>
                      <div>{s.render()}</div>
                    </div>
                  ))}
              </div>
            </section>
          ))}
        </div>
      </main>
    </UiProvider>
  );
}
