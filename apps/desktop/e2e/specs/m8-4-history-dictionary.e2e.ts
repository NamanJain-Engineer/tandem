import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { freshOffline, home, invoke } from "../support/app";
import { TEXT_EDITOR, ensureModel, fixture, hold, mic, resetPlatform, setSetting, waitPasted, words } from "../support/dictate";

// E2E-M8-7 History and dictionary (F-DI-08, F-DI-09).

type Stats = { words: number; takes: number };
type Rule = { id: string };
const correction = (phrase: string, replacement: string, appProfiles: string[] = []) => ({
  id: "",
  kind: "correction",
  replacement,
  phrases: [phrase],
  matchCase: false,
  protect: true,
  spacing: "exact",
  enabled: true,
  lang: "*",
  appProfiles,
  updatedAt: 0,
  deletedAt: null,
});

describe("E2E-M8-7 History and dictionary", () => {
  before(async () => {
    await freshOffline();
    await setSetting("dictate_enabled", true);
    await ensureModel();
  });

  it("E2E-M8-7 search finds a stored take; the CSV has the exact header; clearing keeps the word total", async () => {
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    await waitPasted();
    const found = await invoke<{ takes: Array<{ text: string }> }>("dictate_history", { query: "world", before: null, limit: 50 });
    expect(found.takes.some((t) => words(t.text).includes("world"))).toBe(true);
    const none = await invoke<{ takes: unknown[] }>("dictate_history", { query: "zebracrossing", before: null, limit: 50 });
    expect(none.takes).toEqual([]);

    const csv = join(home(), "history.csv");
    await invoke("dictate_history_export", { path: csv });
    expect(readFileSync(csv, "utf8").split("\n")[0]).toBe("timestamp,text,word_count,duration_seconds,model");

    const before = await invoke<Stats>("dictate_stats");
    await invoke("dictate_history_clear");
    expect((await invoke<{ takes: unknown[] }>("dictate_history", { query: null, before: null, limit: 50 })).takes).toEqual([]);
    expect((await invoke<Stats>("dictate_stats")).words).toBe(before.words);
  });

  it("E2E-M8-7 'tandem'→'Tandem' leaves the URL alone", async () => {
    const rule = await invoke<Rule>("dictionary_save", { rule: correction("tandem", "Tandem") });
    try {
      const out = await invoke<string>("dictionary_test", { sample: "use tandem, https://tandem.dev", appId: null, lang: null });
      expect(out).toBe("use Tandem, https://tandem.dev");
    } finally {
      await invoke("dictionary_delete", { ids: [rule.id] });
    }
  });

  it("E2E-M8-7 an app-scoped rule applies only in that app", async () => {
    const rule = await invoke<Rule>("dictionary_save", { rule: correction("world", "WORLD", ["org.gnome.Terminal"]) });
    try {
      expect(await invoke<string>("dictionary_test", { sample: "hello world", appId: "org.gnome.Terminal", lang: null })).toBe("hello WORLD");
      expect(await invoke<string>("dictionary_test", { sample: "hello world", appId: TEXT_EDITOR.appId, lang: null })).toBe("hello world");
      // And in a real take over the text editor, it stays out.
      await resetPlatform();
      await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
      await hold(2_500);
      const view = await waitPasted();
      expect(view.pasted[0] ?? "").not.toContain("WORLD");
    } finally {
      await invoke("dictionary_delete", { ids: [rule.id] });
    }
  });

  it("E2E-M8-7 a JSON export imports back", async () => {
    const rule = await invoke<Rule>("dictionary_save", { rule: correction("vibe zeo", "Vibezeo") });
    const file = join(home(), "dictionary.json");
    await invoke("dictionary_export", { path: file, format: "json" });
    await invoke("dictionary_delete", { ids: [rule.id] });
    writeFileSync(file, readFileSync(file, "utf8"));
    const preview = await invoke<{ previewId: string; added: unknown[] }>("dictionary_import_preview", { path: file, format: "json" });
    expect(preview.added.length).toBeGreaterThan(0);
    await invoke("dictionary_import_commit", { previewId: preview.previewId });
    expect(await invoke<string>("dictionary_test", { sample: "vibe zeo", appId: null, lang: null })).toBe("Vibezeo");
  });
});
