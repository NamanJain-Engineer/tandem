import { $, $$, browser } from "@wdio/globals";
import {
  addFolder,
  clickAllow,
  freshOffline,
  invoke,
  paneIds,
  sendToPane,
  split,
  textOf,
  waitForPaneText,
  waitForStatus,
} from "../support/app";

type Fixture = { id: string; title: string; status: string; engineId?: string; approval?: string };
const fixture = (f: Fixture) =>
  invoke<{ id: string } | null>("test_fixture_agent", {
    agent: { engineId: "claude", workspaceId: null, preview: null, approval: null, ...f },
  });

const card = (key: string) => `[data-testid="agent-card"][data-key="${key}"]`;
const cardKeys = () =>
  browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="agent-card"]')).map(
      (el) => el.dataset.key ?? "",
    ),
  );

async function openDashboard(): Promise<void> {
  if (
    !(await $('[data-testid="sidebar"]')
      .isDisplayed()
      .catch(() => false))
  )
    await $('[data-testid="sidebar-toggle"]').click();
  await $('[data-testid="sidebar-pill-dashboard"]').click();
  await $('[data-testid="dashboard"]').waitForExist({ timeout: 10_000 });
}

// E2E-M5-4 Dashboard (F-DB-01) with the fixture source.
describe("E2E-M5-4 Dashboard", () => {
  before(async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m5-4-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  it("F-DB-01.1 3 working, 1 waiting and 2 idle read 6 agents with matching counts", async () => {
    await fixture({ id: "w1", title: "api refactor", status: "working" });
    await fixture({ id: "w2", title: "billing tests", status: "working" });
    await fixture({ id: "w3", title: "docs pass", status: "working" });
    await fixture({ id: "q1", title: "deploy", status: "waiting", approval: "pnpm migrate" });
    await fixture({ id: "i1", title: "api client", status: "idle" });
    await fixture({ id: "i2", title: "changelog", status: "idle" });
    await openDashboard();
    await browser.waitUntil(async () => (await textOf('[data-testid="dashboard-count"]')) === "6 agents", {
      timeout: 5_000,
    });
    // innerText breaks between the subtitle's buttons.
    expect((await textOf('[data-testid="dashboard-subtitle"]')).replace(/\s+/g, " ")).toBe(
      "1 waiting for you · 3 working · 0 done · 2 idle",
    );
  });

  it("F-DB-01.2 Close idle with the search 'api' closes only the matching idle agent", async () => {
    await $('[data-testid="dashboard-search"]').setValue("api");
    await browser.waitUntil(async () => (await cardKeys()).length === 2, {
      timeout: 5_000,
      timeoutMsg: `cards: ${await cardKeys()}`,
    });
    await $('[data-testid="close-idle"]').click();
    await $("button=Close 1 agent").click();
    await browser.waitUntil(async () => !(await cardKeys()).includes("fixture:i1"), { timeout: 5_000 });
    // Esc in the search clears it.
    await $('[data-testid="dashboard-search"]').click();
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await cardKeys()).length === 5, {
      timeout: 5_000,
      timeoutMsg: `cards: ${await cardKeys()}`,
    });
    expect(await cardKeys()).toContain("fixture:i2");
  });

  it("F-DB-01.3 Allow once on the waiting card resolves the approval and the agent proceeds", async () => {
    await clickAllow(`${card("fixture:q1")} [data-testid="card-allow"]`);
    await browser.waitUntil(
      async () => (await $(card("fixture:q1")).getAttribute("data-status")) === "working",
      { timeout: 5_000 },
    );
    expect(await invoke<unknown[]>("approvals_pending")).toEqual([]);
  });

  it("F-DB-01.5 a status change reaches the card within 500 ms", async () => {
    const t0 = Date.now();
    await fixture({ id: "w2", title: "billing tests", status: "done" });
    await browser.waitUntil(
      async () => (await $(card("fixture:w2")).getAttribute("data-status")) === "done",
      { timeout: 2_000, interval: 25 },
    );
    expect(Date.now() - t0).toBeLessThan(500 + 300); // + one WebDriver round trip
  });

  it("F-DB-01.4 clicking a card from Thread mode focuses its pane in Code mode and flashes it for 1.2 s", async () => {
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    await sendToPane(agent, "hello\r");
    await waitForStatus(agent, "done");
    await browser.execute(() => {
      const radio = Array.from(document.querySelectorAll<HTMLElement>('[role="radio"]')).find(
        (el) => el.textContent?.trim() === "Thread",
      );
      radio?.click();
    });
    await openDashboard();
    await $(card(`pane:${agent}`)).click();
    await browser.waitUntil(
      async () =>
        (await $(`[data-testid="pane"][data-pane-id="${agent}"]`).getAttribute("data-flash")) === "true",
      { timeout: 5_000 },
    );
    const started = Date.now();
    expect(
      await browser.execute(() =>
        Array.from(document.querySelectorAll('[role="radio"][aria-checked="true"]')).map((el) =>
          el.textContent?.trim(),
        ),
      ),
    ).toContain("Code");
    await browser.waitUntil(
      async () =>
        (await $(`[data-testid="pane"][data-pane-id="${agent}"]`).getAttribute("data-flash")) !== "true",
      { timeout: 3_000 },
    );
    const lasted = Date.now() - started;
    expect(lasted).toBeGreaterThan(800);
    expect(lasted).toBeLessThan(1_900);
    expect(
      await $$('[data-testid="pane"][data-focused="true"]').map((el) => el.getAttribute("data-pane-id")),
    ).toEqual([agent]);
  });
});
