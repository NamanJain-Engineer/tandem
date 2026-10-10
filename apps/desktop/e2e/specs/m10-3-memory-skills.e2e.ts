import { $, $$, browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { freshOffline, invoke, mod } from "../support/app";
import { argAfter, chatSpawns, makeTeammate, newChat, sendAndWait, type Teammate } from "../support/agents";

/** Clicks the teammate page's tab whose label starts with `name`. */
async function openTab(name: string): Promise<void> {
  const clicked = await browser.execute((n: string) => {
    const tab = Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid="teammate-page"] [role="tab"]'),
    ).find((el) => (el.textContent ?? "").trim().startsWith(n));
    // Radix tabs switch on mousedown, not click.
    tab?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    tab?.click();
    return !!tab;
  }, name);
  expect(clicked).toBe(true);
}

/** Picks the Memory tab's List / Raw view. */
async function pick(view: string): Promise<void> {
  const clicked = await browser.execute((v: string) => {
    const radio = Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid="memory-tab"] [role="radio"]'),
    ).find((el) => (el.textContent ?? "").trim() === v);
    radio?.click();
    return !!radio;
  }, view);
  expect(clicked).toBe(true);
}

// E2E-M10-3 Memory and skills (F-AG-07, F-AG-09): the agent's memory writes
// are capped and credential-free; the Memory tab's List and Raw views don't
// touch the file; library skills arrive valid; invalid ones stay out of the
// briefing.
describe("E2E-M10-3 memory and skills", () => {
  let mira: Teammate;
  let chat = "";
  let notes = "";

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    mira = await makeTeammate("Mira");
    notes = join(mira.home, "memory", "NOTES.md");
    writeFileSync(notes, "- Ships on Thursdays.\n");
    chat = await newChat(mira.id);
  });

  it("F-AG-07.1 an agent write over the cap is rejected and the file is unchanged", async function () {
    this.timeout(30_000);
    const big = "x".repeat(5000);
    const replies = await sendAndWait(chat, `mcp tandem__memory_write {"file":"notes","markdown":"${big}"}`);
    expect(replies.join(" ")).toContain("Condense it and try again.");
    expect(readFileSync(notes, "utf8")).toBe("- Ships on Thursdays.\n");
  });

  it('F-AG-07.2 "token=ghp_…" is rejected', async function () {
    this.timeout(30_000);
    const replies = await sendAndWait(
      chat,
      'mcp tandem__memory_write {"file":"notes","markdown":"- deploy with token=ghp_abcdefghijklmnopqrstuvwxyz0123456789\\n"}',
    );
    expect(replies.join(" ")).toContain("Memory can't store credentials");
    expect(readFileSync(notes, "utf8")).toBe("- Ships on Thursdays.\n");
  });

  it("F-AG-07.3 List → Raw → List leaves NOTES.md byte for byte", async function () {
    this.timeout(30_000);
    const odd =
      "﻿# Notes\r\n\r\n- Release branch is cut on Thursdays.\r\n  Never on Fridays.\r\n- Prefers pnpm.\r\ntrailing text";
    writeFileSync(notes, odd);
    const mtime = statSync(notes).mtimeMs;
    await browser.keys([mod(), "1"]);
    await $(`[data-teammate-id="${mira.id}"]`).click();
    await openTab("Memory");
    await $('[data-testid="memory-notes"]').waitForDisplayed({ timeout: 10_000 });
    expect(await $$('[data-testid="memory-note"]').length).toBe(2);
    await pick("Raw");
    await $('[data-testid="memory-raw"]').waitForDisplayed({ timeout: 5_000 });
    await pick("List");
    await $('[data-testid="memory-notes"]').waitForDisplayed({ timeout: 5_000 });
    expect(readFileSync(notes, "utf8")).toBe(odd);
    expect(statSync(notes).mtimeMs).toBe(mtime);
  });

  it("F-AG-09.1 release-notes from the library is a valid skill; invalid front matter is listed with an error and left out of the briefing", async function () {
    this.timeout(60_000);
    await openTab("Skills");
    await $('[data-testid="skills-library"]').click();
    await $('[data-testid="library-add-release-notes"]').click();
    const row = await $('[data-testid="skill-row"][data-slug="release-notes"]');
    await row.waitForExist({ timeout: 10_000 });
    expect(await row.getAttribute("data-valid")).toBe("true");
    expect(existsSync(join(mira.home, "skills", "release-notes", "SKILL.md"))).toBe(true);
    await browser.keys("Escape");
    // A broken skill the user wrote by hand.
    mkdirSync(join(mira.home, "skills", "broken-skill"), { recursive: true });
    writeFileSync(
      join(mira.home, "skills", "broken-skill", "SKILL.md"),
      "---\nname: [broken, skill]\n---\nDo it.\n",
    );
    const list = await invoke<Array<{ slug: string; valid: boolean; error: string | null }>>("skills_list", {
      teammateId: mira.id,
    });
    const broken = list.find((s) => s.slug === "broken-skill");
    expect(broken?.valid).toBe(false);
    expect(broken?.error).toBeTruthy();
    // The next chat's briefing indexes release-notes and not the broken one.
    const before = (await chatSpawns()).length;
    const next = await newChat(mira.id);
    await sendAndWait(next, "hello");
    const spawn = (await chatSpawns()).slice(before).at(-1);
    const brief = readFileSync(argAfter(spawn?.argv ?? [], "--append-system-prompt-file") ?? "", "utf8");
    const index = brief.split("<skills>")[1]?.split("</skills>")[0] ?? "";
    expect(index).toContain("release-notes");
    expect(index).not.toContain("broken-skill");
  });
});
