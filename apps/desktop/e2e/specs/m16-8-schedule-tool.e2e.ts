import { $, browser } from "@wdio/globals";
import { freshUser, invoke, mod, planFooter, setPlan } from "../support/app";
import { makeTeammate, newChat, sendAndWait, type Teammate } from "../support/agents";

type Routine = { id: string; name: string; access: string; createdByTeammateId: string | null };

async function openSettings(id: string): Promise<void> {
  await browser.keys([mod(), "1"]);
  await $(`[data-teammate-id="${id}"]`).click();
  await $('[data-testid="teammate-page"]').waitForDisplayed({ timeout: 10_000 });
  const clicked = await browser.execute(() => {
    const tab = Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid="teammate-page"] [role="tab"]'),
    ).find((el) => (el.textContent ?? "").trim().startsWith("Settings"));
    tab?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    tab?.click();
    return !!tab;
  });
  expect(clicked).toBe(true);
}

// F-RT-05 (M16) Schedule tool for teammates: without "May create routines"
// a teammate's chat has no schedule tools; with it, the tools appear, Full
// access is rejected, and a routine it makes is its own, Edits, marked
// "created by {name}", and the user is told.
describe("F-RT-05 schedule tool for teammates", () => {
  let rhea: Teammate;
  let chat = "";

  before(async function () {
    this.timeout(180_000);
    const { email } = await freshUser("rt05");
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    rhea = await makeTeammate("Rhea");
    chat = await newChat(rhea.id);
  });

  it("F-RT-05.1 a teammate without the permission gets no schedule tools; turning it on adds them", async function () {
    this.timeout(90_000);
    const before = (await sendAndWait(chat, "tools")).join(" ");
    expect(before).not.toContain("tandem__schedule_");
    await openSettings(rhea.id);
    await $('[data-testid="teammate-settings"] [aria-label="May create routines"]').click();
    await $('[data-testid="settings-save"]').click();
    await browser.waitUntil(
      async () => browser.execute(() => !document.querySelector('[data-testid="save-bar"]')),
      { timeout: 10_000, timeoutMsg: "the Save bar stayed" },
    );
    const after = (await sendAndWait(chat, "tools")).join(" ");
    for (const t of ["list", "create", "update", "delete"]) expect(after).toContain(`tandem__schedule_${t}`);
  });

  it("F-RT-05.2 schedule_create with Full is rejected; a valid one is the teammate's own, and the user is told", async function () {
    this.timeout(90_000);
    const args = {
      name: "Changelog",
      instruction: "Add yesterday's merged work to CHANGELOG.md.",
      cron: "0 7 * * 1-5",
      timezone: "UTC",
    };
    const refused = (
      await sendAndWait(chat, `mcp tandem__schedule_create ${JSON.stringify({ ...args, access: "full" })}`)
    ).join(" ");
    expect(refused).toContain("can't have Full access");
    expect(await invoke<unknown[]>("routines_list")).toHaveLength(0);
    await sendAndWait(chat, `mcp tandem__schedule_create ${JSON.stringify(args)}`);
    const list = await invoke<Array<{ routine: Routine }>>("routines_list");
    expect(list).toHaveLength(1);
    const r = list[0]!.routine;
    expect(r.name).toBe("Changelog");
    expect(r.access).toBe("edits");
    expect(r.createdByTeammateId).toBe(rhea.id);
    const notes = await invoke<{ items: Array<{ title: string }> }>("notifications_list", {});
    expect(notes.items.some((n) => n.title === "Rhea scheduled a routine: Changelog")).toBe(true);
    // The Routines page marks it.
    await $('[data-testid="agent-footer-routines"]').click();
    const row = `[data-testid="routine-row"][data-id="${r.id}"]`;
    await $(row).waitForDisplayed({ timeout: 10_000 });
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          (s) => document.querySelector(s)?.textContent ?? "",
          `${row} [data-testid="routine-created-by"]`,
        )) === "created by Rhea",
      { timeout: 10_000, timeoutMsg: "no created-by mark" },
    );
  });
});
