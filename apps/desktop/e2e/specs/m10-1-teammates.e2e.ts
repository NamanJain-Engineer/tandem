import { $, browser } from "@wdio/globals";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { checkAgain, fakeEngine, freshOffline, home, invoke, mod, relaunch } from "../support/app";

type Summary = { teammate: { id: string; name: string; isLead: boolean; pinned: boolean; engineId: string } };

/** What a teammate's avatar draws: its colours and initials. */
async function avatarOf(id: string): Promise<string> {
  return String(
    await browser.execute((tid: string) => {
      const img = document.querySelector<HTMLElement>(`[data-teammate-id="${tid}"] [role="img"]`);
      return img ? `${img.style.background}|${img.textContent ?? ""}` : "";
    }, id),
  );
}

async function openSheet(): Promise<void> {
  await browser.keys([mod(), "1"]);
  await $('[data-testid="teammate-new"]').click();
  await $('[data-testid="teammate-name-input"]').waitForDisplayed({ timeout: 10_000 });
}

// E2E-M10-1 Teammates (F-AG-01, F-AG-04): a fresh profile has a pinned
// Lead; names are unique ignoring case; Create waits for the engine to be
// installed; an avatar looks the same after a relaunch.
describe("E2E-M10-1 teammates", () => {
  let scoutId = "";

  before(async function () {
    this.timeout(60_000);
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    await freshOffline();
  });

  // Later specs run without this Codex.
  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).not.toContain("codex");
  });

  it("F-AG-01.3 a fresh profile has Lead, pinned as the hero", async function () {
    this.timeout(30_000);
    await browser.keys([mod(), "1"]);
    const hero = await $('[data-testid="teammate-hero"]');
    await hero.waitForDisplayed({ timeout: 10_000 });
    expect(await hero.getAttribute("title")).toBe("Lead");
    const list = await invoke<Summary[]>("teammate_list");
    expect(list.filter((s) => s.teammate.isLead && s.teammate.pinned)).toHaveLength(1);
  });

  it("F-AG-04 Create stays disabled for an engine that isn't installed until Recheck finds it", async function () {
    this.timeout(60_000);
    await openSheet();
    await $('[data-testid="teammate-name-input"]').setValue("Scout");
    await $('[data-testid="teammate-brief"]').setValue("You keep the changelog.");
    const codex = await $('[data-testid="engine-tile"][data-engine="codex"]');
    await browser.waitUntil(async () => codex.isExisting(), {
      timeout: 10_000,
      timeoutMsg: `no Codex tile; tiles: ${JSON.stringify(
        await browser.execute(() =>
          Array.from(document.querySelectorAll<HTMLElement>('[data-testid="engine-tile"]')).map(
            (e) => e.dataset.engine,
          ),
        ),
      )}`,
    });
    expect(await codex.getAttribute("data-installed")).toBe("false");
    await codex.click();
    const create = await $('[data-testid="teammate-create"]');
    expect(await create.isEnabled()).toBe(false);
    fakeEngine("codex", join(home(), "fakebin"));
    await $('[data-testid="engines-recheck"]').click();
    await browser.waitUntil(async () => (await codex.getAttribute("data-installed")) === "true", {
      timeout: 30_000,
      timeoutMsg: "Recheck never found the fake codex",
    });
    await browser.waitUntil(async () => create.isEnabled(), {
      timeout: 10_000,
      timeoutMsg: "Create stayed disabled",
    });
    await create.click();
    await browser.waitUntil(
      async () => (await invoke<Summary[]>("teammate_list")).some((s) => s.teammate.name === "Scout"),
      { timeout: 10_000, timeoutMsg: "Scout wasn't created" },
    );
    const scout = (await invoke<Summary[]>("teammate_list")).find((s) => s.teammate.name === "Scout");
    expect(scout?.teammate.engineId).toBe("codex");
    scoutId = scout?.teammate.id ?? "";
  });

  it('F-AG-01.1 "scout" is refused: the name is already used', async function () {
    this.timeout(30_000);
    await openSheet();
    await $('[data-testid="teammate-name-input"]').setValue("scout");
    await $('[data-testid="teammate-brief"]').setValue("Another one.");
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.body.innerText)).includes(
          "You already have a teammate called scout.",
        ),
      { timeout: 10_000, timeoutMsg: "no name-taken message" },
    );
    expect(await $('[data-testid="teammate-create"]').isEnabled()).toBe(false);
    // The core refuses it too, whatever the UI does.
    const refused = await invoke("teammate_create", {
      input: { name: "SCOUT", brief: "x", engineId: "claude", places: [] },
    }).then(
      () => "created",
      (e: Error) => e.message,
    );
    expect(refused).toContain('"code":"conflict"');
    expect(refused).toContain('"message":"Name already used"');
    await browser.keys("Escape");
  });

  it("F-AG-01.2 the avatar is the same after a relaunch", async function () {
    this.timeout(90_000);
    await browser.keys([mod(), "1"]);
    await $(`[data-teammate-id="${scoutId}"]`).waitForDisplayed({ timeout: 10_000 });
    const before = await avatarOf(scoutId);
    expect(before).toContain("linear-gradient(");
    await relaunch();
    await browser.keys([mod(), "1"]);
    await $(`[data-teammate-id="${scoutId}"]`).waitForDisplayed({ timeout: 20_000 });
    expect(await avatarOf(scoutId)).toBe(before);
  });
});
