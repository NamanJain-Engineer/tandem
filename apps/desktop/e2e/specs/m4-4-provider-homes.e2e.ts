import { browser } from "@wdio/globals";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, home, invoke, paneIds, waitForPaneText } from "../support/app";

type Account = { id: string; engineId: string; label: string; homePath: string; isDefault: boolean };
type SpawnRecord = {
  argv: string[];
  envKeys: string[];
  secretEnvKeys: string[];
  providerHome: string | null;
  cwd: string;
};

// E2E-M4-4 Provider homes (F-AC-01, F-EN-04): two Claude accounts, each in
// its own private home.
describe("E2E-M4-4 provider homes", () => {
  it("F-AC-01.1 F-EN-04.1 two accounts launch with distinct 0700 homes, no secrets in env, user keys kept", async () => {
    await freshOffline();
    const root = `/tmp/tandem-e2e-m4-4-${Date.now()}`;
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list");
    const tabId = ws?.tabs[0]?.id ?? "";
    const work = await invoke<Account>("test_account_create", {
      engineId: "claude",
      label: `Work ${Date.now()}`,
    });
    // A key the user put in the second account's settings before Tandem adds its hooks.
    mkdirSync(work.homePath, { recursive: true });
    writeFileSync(join(work.homePath, "settings.json"), JSON.stringify({ model: "opus", theme: "dark" }));
    const before = (await invoke<SpawnRecord[]>("test_spawn_log")).length;
    const a = await invoke<{ id: string }>("pane_create", {
      tabId,
      kind: "agent",
      engineId: "claude",
      cwd: null,
    });
    const b = await invoke<{ id: string }>("pane_create", {
      tabId,
      kind: "agent",
      engineId: "claude",
      cwd: null,
      accountId: work.id,
    });
    await invoke("layout_save", {
      tabId,
      tree: {
        type: "split",
        dir: "row",
        ratio: 0.5,
        a: { type: "leaf", paneId: a.id },
        b: { type: "leaf", paneId: b.id },
      },
      activePaneId: a.id,
      maximizedPaneId: null,
    });
    await browser.refresh();
    // The tab's first shell stays alongside the two agents.
    await browser.waitUntil(
      async () => {
        const now = await paneIds();
        return now.includes(a.id) && now.includes(b.id);
      },
      { timeout: 30_000, timeoutMsg: `agent panes didn't open: ${JSON.stringify(await paneIds())}` },
    );
    for (const id of [a.id, b.id]) await waitForPaneText(id, "Fake Claude Code");

    const spawns = (await invoke<SpawnRecord[]>("test_spawn_log"))
      .slice(before)
      .filter((s) => s.providerHome);
    const homes = [...new Set(spawns.map((s) => s.providerHome))];
    expect(homes).toHaveLength(2);
    const providers = join(home(), "providers", "claude");
    for (const h of homes) {
      expect(h?.startsWith(providers)).toBe(true);
      expect(statSync(h ?? "").mode & 0o777).toBe(0o700);
    }
    expect(homes).toContain(work.homePath);
    for (const s of spawns) {
      expect(s.envKeys).toContain("CLAUDE_CONFIG_DIR");
      expect(s.secretEnvKeys).toEqual([]);
    }
    // The user's key survived, next to Tandem's hooks.
    const settings = JSON.parse(readFileSync(join(work.homePath, "settings.json"), "utf8")) as {
      model?: string;
      hooks?: Record<string, unknown>;
    };
    expect(settings.model).toBe("opus");
    expect(Object.keys(settings.hooks ?? {})).toContain("Stop");
    const accounts = await invoke<Account[]>("accounts_list");
    expect(accounts.filter((x) => x.engineId === "claude").length).toBeGreaterThanOrEqual(2);
  });
});
