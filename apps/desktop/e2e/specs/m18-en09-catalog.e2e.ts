import { browser } from "@wdio/globals";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { addFolder, checkAgain, fakeEngine, freshOffline, home, invoke, paneIds, split, waitForPaneText } from "../support/app";

type View = { spec: { id: string; note: string | null }; chatCapable: boolean };
type Spawn = { argv: string[]; envKeys: string[] };

// E2E-M18-EN09 Engine catalog (F-EN-09): Antigravity CLI and Kiro CLI are
// detected and launch as terminal engines; Kiro gets its account's home;
// neither is a Thread engine; Gemini CLI keeps its note.
describe("E2E-M18-EN09 engine catalog", function () {
  this.timeout(180_000);
  const bin = join(home(), "fakebin");

  before(async () => {
    fakeEngine("agy", bin);
    fakeEngine("kiro-cli", bin);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-en09-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  after(async () => {
    rmSync(join(bin, "agy"), { force: true });
    rmSync(join(bin, "kiro-cli"), { force: true });
    await checkAgain().catch(() => undefined);
  });

  it("F-EN-09.1 agy and kiro-cli are detected and launch in panes, Kiro with KIRO_HOME", async () => {
    const report = await checkAgain();
    const found = report.found.map((f) => f.engineId);
    expect(found).toContain("antigravity");
    expect(found).toContain("kiro");
    const kiro = await split("right", "kiro");
    await waitForPaneText(kiro, "Fake kiro-cli (test engine)");
    const spawns = await invoke<Spawn[]>("test_spawn_log");
    expect(spawns.some((s) => s.envKeys.includes("KIRO_HOME"))).toBe(true);
    const agy = await split("down", "antigravity");
    await waitForPaneText(agy, "Fake agy (test engine)");
  });

  it("F-EN-09.2 and F-EN-09.3 Gemini CLI keeps its note, and neither new engine is a Thread engine", async () => {
    const views = await invoke<View[]>("engines_list");
    const get = (id: string) => views.find((v) => v.spec.id === id);
    expect(get("gemini")?.spec.note).toContain("Personal Google accounts use Antigravity CLI");
    expect(get("antigravity")?.chatCapable).toBe(false);
    expect(get("kiro")?.chatCapable).toBe(false);
  });
});
