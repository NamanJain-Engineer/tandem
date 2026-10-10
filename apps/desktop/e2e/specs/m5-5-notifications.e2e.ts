import { $, browser } from "@wdio/globals";
import {
  addFolder,
  freshOffline,
  invoke,
  paneIds,
  sendToPane,
  split,
  waitForPaneText,
  waitForStatus,
} from "../support/app";

type OsCall = { kind: string; title: string; body: string; source: { type: string; paneId?: string } };
type Page = {
  items: Array<{
    id: string;
    kind: string;
    state: string;
    dedupeKey: string | null;
    source: { type: string; paneId?: string };
  }>;
};

const calls = (clear = false) => invoke<OsCall[]>("test_notifier_calls", { clear });
const doneCalls = async () => (await calls()).filter((c) => c.kind === "done");

async function modeIs(name: string): Promise<boolean> {
  const checked = await browser.execute(() =>
    Array.from(document.querySelectorAll('[role="radio"][aria-checked="true"]')).map((el) =>
      el.textContent?.trim(),
    ),
  );
  return checked.includes(name);
}

async function switchMode(name: string): Promise<void> {
  await browser.execute((n: string) => {
    Array.from(document.querySelectorAll<HTMLElement>('[role="radio"]'))
      .find((el) => el.textContent?.trim() === n)
      ?.click();
  }, name);
  await browser.waitUntil(() => modeIs(name), { timeout: 5_000 });
}

// E2E-M5-5 Notifications (F-DB-02, F-DB-03) with the recording notifier.
describe("E2E-M5-5 notifications", () => {
  let agent = "";

  before(async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m5-5-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    await calls(true);
  });

  after(async () => {
    await invoke("test_set_window_focused", { focused: null });
    await invoke("settings_set", { key: "notify_types", value: {} });
  });

  it("F-DB-03.1 with the app unfocused an agent finishing makes one OS notification; clicking it focuses the source", async () => {
    await invoke("test_set_window_focused", { focused: false });
    await sendToPane(agent, "hello\r");
    await waitForStatus(agent, "done");
    await browser.waitUntil(async () => (await doneCalls()).length === 1, {
      timeout: 5_000,
      timeoutMsg: `calls: ${JSON.stringify(await calls())}`,
    });
    const [call] = await doneCalls();
    expect(call?.source).toEqual({ type: "pane", paneId: agent });
    await browser.pause(1_000);
    expect(await doneCalls()).toHaveLength(1);
    // Clicking it from another mode lands on the pane.
    await invoke("test_set_window_focused", { focused: null });
    await switchMode("Thread");
    const index = (await calls()).findIndex((c) => c.kind === "done");
    await invoke("test_notifier_click", { index });
    await browser.waitUntil(() => modeIs("Code"), { timeout: 5_000 });
    await browser.waitUntil(
      async () =>
        (await $(`[data-testid="pane"][data-pane-id="${agent}"]`).getAttribute("data-flash")) === "true",
      { timeout: 5_000 },
    );
  });

  it("F-DB-03.2 with the done toggle off nothing is sent", async () => {
    await invoke("settings_set", { key: "notify_types", value: { "done.system": false } });
    await invoke("test_set_window_focused", { focused: false });
    await calls(true);
    await sendToPane(agent, "again\r");
    await waitForPaneText(agent, "you said: again");
    await waitForStatus(agent, "done");
    await browser.pause(1_000);
    expect(await doneCalls()).toHaveLength(0);
    // The bell still lists it.
    const page = await invoke<Page>("notifications_list", {});
    expect(page.items.filter((n) => n.kind === "done").length).toBeGreaterThanOrEqual(2);
  });

  it("F-DB-02.1 an approval answered on the Dashboard resolves its notification", async () => {
    await invoke("test_fixture_agent", {
      agent: {
        id: "ap",
        title: "deploy",
        status: "waiting",
        engineId: "claude",
        workspaceId: null,
        preview: null,
        approval: "pnpm migrate",
      },
    });
    let approval: Page["items"][number] | undefined;
    await browser.waitUntil(
      async () => {
        approval = (await invoke<Page>("notifications_list", {})).items.find(
          (n) => n.kind === "approval" && n.source.type === "fixture",
        );
        return approval !== undefined;
      },
      { timeout: 5_000 },
    );
    expect(approval?.state).toBe("unread");
    if (
      !(await $('[data-testid="sidebar"]')
        .isDisplayed()
        .catch(() => false))
    )
      await $('[data-testid="sidebar-toggle"]').click();
    await $('[data-testid="sidebar-pill-dashboard"]').click();
    await $('[data-testid="agent-card"][data-key="fixture:ap"] [data-testid="card-allow"]').click();
    await browser.waitUntil(
      async () =>
        (await invoke<Page>("notifications_list", {})).items.find((n) => n.id === approval?.id)?.state ===
        "resolved",
      { timeout: 5_000 },
    );
  });

  it("F-DB-02.2 clicking a done item from Thread mode switches to Code and focuses the pane", async () => {
    await switchMode("Thread");
    await $('[data-testid="notifications-bell"]').click();
    await $('[data-testid="notifications-popover"]').waitForExist({ timeout: 5_000 });
    await browser.execute(() => {
      const radio = Array.from(
        document.querySelectorAll<HTMLElement>('[data-testid="notifications-popover"] [role="radio"]'),
      ).find((el) => el.textContent?.trim() === "All");
      radio?.click();
    });
    await $('[data-testid="notification"][data-kind="done"]').click();
    await browser.waitUntil(() => modeIs("Code"), { timeout: 5_000 });
    expect(await $(`[data-testid="pane"][data-pane-id="${agent}"]`).getAttribute("data-focused")).toBe(
      "true",
    );
  });
});
