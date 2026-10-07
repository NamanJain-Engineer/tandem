import { $, browser } from "@wdio/globals";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  fakeLog,
  freshOffline,
  home,
  paneIds,
  split,
  waitForPaneText,
} from "../support/app";

const HEADINGS = [
  "## Goal",
  "## What's done",
  "## Current state",
  "## Files touched",
  "## Open problems",
  "## Next steps",
];

/** `empty` makes the fake engines' briefs empty (TANDEM_FAKE_BRIEF). */
const briefMode = (mode: "" | "empty") => writeFileSync(join(home(), "brief.txt"), mode);

/** Everything the fake engines received as bracketed pastes since `mark`. */
function pastes(mark: number): string[] {
  const bytes = fakeLog()
    .slice(mark)
    .filter((l) => l.startsWith("raw: "))
    .map((l) => Buffer.from(l.slice(5), "hex").toString("utf8"))
    .join("");
  const [open, close] = ["\u001b[200~", "\u001b[201~"];
  return bytes
    .split(open)
    .slice(1)
    .map((chunk) => chunk.split(close)[0] ?? "");
}

/** Clicks a menu item by its text once the menu is open. */
async function pick(label: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find(
          (el) => el.textContent?.trim() === l,
        );
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

async function handOff(pane: string): Promise<void> {
  await $(`[data-testid="pane"][data-pane-id="${pane}"] button[aria-label^="Actions for "]`).click();
  await pick("Hand off…");
  await $('[data-testid="handoff-dialog"]').waitForExist({ timeout: 10_000 });
}

/** Text whether or not WebKit counts it as rendered yet (dialogs fade in). */
const text = async (selector: string) => String(await $(selector).getProperty("textContent"));

const summary = async () => String(await $('[data-testid="handoff-summary"]').getProperty("value"));

const box = (pane: string) =>
  browser.execute((id: string) => {
    const r = document.querySelector(`[data-testid="pane"][data-pane-id="${id}"]`)?.getBoundingClientRect();
    return r ? { x: r.left, y: r.top } : null;
  }, pane);

// E2E-M6-5: handoff between engines (F-CD-16, UI-13).
describe("E2E-M6-5 Handoff", () => {
  let codex = "";

  before(() => {
    mkdirSync(join(home(), "fakebin"), { recursive: true });
    fakeEngine("codex", join(home(), "fakebin"));
    briefMode("");
  });
  after(async () => {
    briefMode("");
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    await checkAgain();
  });

  it("F-CD-16.1 from a Codex pane to Claude: Claude opens to its right and its first input has the template headings", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m6-5-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await checkAgain();
    codex = await split("right", "codex");
    await waitForPaneText(codex, "Fake Codex");

    const asked = fakeLog().length;
    await handOff(codex);
    expect(await text('[data-testid="handoff-dialog"]')).toContain(
      "Vibezeo will ask Codex to write a handoff summary, then start Claude Code with it.",
    );
    // Codex is asked for the brief and writes it between the markers.
    await browser.waitUntil(async () => (await summary()).includes("## Next steps"), {
      timeout: 60_000,
      timeoutMsg: `summary: ${await summary()}`,
    });
    expect(
      fakeLog()
        .slice(asked)
        .some((l) => l.startsWith("line: Write a handoff brief")),
    ).toBe(true);
    expect(await summary()).toContain("Finish the work Fake Codex started.");

    const before = await paneIds();
    const mark = fakeLog().length;
    await $('[data-testid="handoff-start"]').click();
    await $('[data-testid="handoff-dialog"]').waitForExist({ reverse: true, timeout: 20_000 });
    let claude = "";
    await browser.waitUntil(
      async () => {
        claude = (await paneIds()).find((id) => !before.includes(id)) ?? "";
        return claude !== "";
      },
      { timeout: 20_000 },
    );
    // Split right of the source: same row, further right.
    const [src, dst] = [await box(codex), await box(claude)];
    expect(dst && src && dst.x > src.x && Math.abs(dst.y - src.y) < 2).toBe(true);
    await browser.waitUntil(async () => pastes(mark).some((p) => p.includes("## Goal")), {
      timeout: 60_000,
      timeoutMsg: `first inputs: ${JSON.stringify(pastes(mark))}`,
    });
    const first = pastes(mark).find((p) => p.includes("## Goal")) ?? "";
    for (const h of HEADINGS) expect(first).toContain(h);
    // The source says where the work went, in plain sight.
    const note = $(`[data-testid="pane"][data-pane-id="${codex}"] [data-testid="handoff-note"]`);
    expect(await note.isDisplayed()).toBe(true);
    expect(await text(`[data-testid="pane"][data-pane-id="${codex}"] [data-testid="handoff-note"]`)).toMatch(
      /^Handed off to .+ \(Claude Code\)$/,
    );
  });

  it("F-CD-16.3 with an empty brief the fallback is used and the target still launches", async () => {
    briefMode("empty");
    const asked = fakeLog().length;
    await handOff(codex);
    await browser.waitUntil(
      async () => (await summary()).includes("The last lines of the previous session"),
      {
        timeout: 60_000,
        timeoutMsg: `summary: ${await summary()}`,
      },
    );
    await $('[data-testid="handoff-fallback"]').waitForExist({ timeout: 5_000 });
    // The empty reply was refused, then Claude ran headless (and wrote nothing).
    expect(
      fakeLog()
        .slice(asked)
        .some((l) => /^headless: claude read \d+ bytes$/.test(l)),
    ).toBe(true);
    for (const h of HEADINGS) expect(await summary()).toContain(h);

    const before = await paneIds();
    const mark = fakeLog().length;
    await $('[data-testid="handoff-start"]').click();
    await $('[data-testid="handoff-dialog"]').waitForExist({ reverse: true, timeout: 20_000 });
    await browser.waitUntil(async () => (await paneIds()).some((id) => !before.includes(id)), {
      timeout: 20_000,
    });
    await browser.waitUntil(
      async () => pastes(mark).some((p) => p.includes("The last lines of the previous session")),
      {
        timeout: 60_000,
        timeoutMsg: `first inputs: ${JSON.stringify(pastes(mark))}`,
      },
    );
  });
});
