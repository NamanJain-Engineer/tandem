import { $, browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { fakeLog, freshOffline, invoke, mod, paneIds } from "../support/app";
import { type Teammate } from "../support/agents";

// E2E-M16-5 Extras, teammates (F-AG-10, F-AG-13): "Also install for …"
// creates and removes Vibezeo's copy of a skill in Claude Code's own skills
// folder, a same-named skill already there is left untouched and the row
// says so; and a teammate launched in a terminal gets its brief as the
// pane's first input.

type Account = { id: string; engineId: string; label: string; homePath: string };
type Pane = { id: string; tabId: string; humanName: string | null; engineId: string | null; cwd: string | null };

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

async function openTeammate(id: string): Promise<void> {
  await browser.keys([mod(), "1"]);
  const row = $(`[data-teammate-id="${id}"]`);
  await row.waitForClickable({ timeout: 10_000 });
  await row.click();
  await $(`[data-testid="teammate-page"][data-teammate-id="${id}"]`).waitForDisplayed({ timeout: 10_000 });
}

const row = (slug: string) => $(`[data-testid="skill-row"][data-slug="${slug}"]`);

/** Ticks or unticks "Also install for → `label`" in a skill's menu. */
async function toggleInstall(slug: string, label: string): Promise<void> {
  const trigger = row(slug).$(`button[aria-label="Actions for ${slug}"]`);
  await trigger.waitForClickable({ timeout: 10_000 });
  await trigger.click();
  const item = $(`//*[@role="menuitemcheckbox"][normalize-space()="${label}"]`);
  await item.waitForDisplayed({ timeout: 10_000 });
  await item.click();
}

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

describe("E2E-M16-5 teammate extras", function () {
  this.timeout(180_000);
  let scout: Teammate;
  let account: Account;
  let label = "";
  const place = `/tmp/tandem-e2e-m16-5-${Date.now()}`;
  const BRIEF = "You are Scout. Keep CHANGELOG.md current from merged pull requests.";

  before(async () => {
    await freshOffline();
    mkdirSync(place, { recursive: true });
    scout = await invoke<Teammate>("teammate_create", {
      input: { name: "Scout", brief: BRIEF, engineId: "claude", places: [place] },
    });
    // An engine account of its own, so its skills folder starts empty.
    account = await invoke<Account>("test_account_create", { engineId: "claude", label: "Skills" });
    const claude = (await invoke<Account[]>("accounts_list")).filter((a) => a.engineId === "claude");
    label = claude.length > 1 ? `Claude Code (${account.label})` : "Claude Code";
    for (const slug of ["release-notes", "test-first"])
      await invoke("skill_add_from_library", { teammateId: scout.id, slug });
  });

  it("F-AG-10.1 syncing to Claude creates <provider home>/skills/<name>/SKILL.md, and removing it deletes it", async () => {
    const copy = join(account.homePath, "skills", "release-notes", "SKILL.md");
    expect(existsSync(copy)).toBe(false);
    await openTeammate(scout.id);
    await openTab("Skills");
    await row("release-notes").waitForDisplayed({ timeout: 10_000 });
    await toggleInstall("release-notes", label);
    await browser.waitUntil(() => existsSync(copy), { timeout: 10_000, timeoutMsg: `no ${copy}` });
    expect(readFileSync(copy, "utf8")).toBe(
      readFileSync(join(scout.home, "skills", "release-notes", "SKILL.md"), "utf8"),
    );
    await row("release-notes")
      .$(`[data-testid="skill-synced"][data-account-id="${account.id}"]`)
      .waitForDisplayed({ timeout: 10_000 });

    await toggleInstall("release-notes", label);
    await browser.waitUntil(() => !existsSync(join(account.homePath, "skills", "release-notes")), {
      timeout: 10_000,
      timeoutMsg: "the synced copy is still there",
    });
    await row("release-notes")
      .$('[data-testid="skill-synced"]')
      .waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("F-AG-10.2 a same-name skill the engine already has is left untouched, and the row says so", async () => {
    const theirs = join(account.homePath, "skills", "test-first");
    mkdirSync(theirs, { recursive: true });
    const own = "---\nname: test-first\ndescription: The user's own.\n---\nMine.\n";
    writeFileSync(join(theirs, "SKILL.md"), own);
    await toggleInstall("test-first", label);
    const conflict = row("test-first").$('[data-testid="skill-conflict"]');
    await conflict.waitForExist({ timeout: 10_000 });
    const text = (await browser.execute(
      () => document.querySelector('[data-slug="test-first"] [data-testid="skill-conflict"]')?.textContent ?? "",
    )) as string;
    expect(text).toContain("already has its own skill called test-first");
    expect(readFileSync(join(theirs, "SKILL.md"), "utf8")).toBe(own);
    expect(existsSync(join(theirs, ".tandem-sync"))).toBe(false);
    // Removing the teammate's skill doesn't touch it either.
    await invoke("skill_remove", { teammateId: scout.id, slug: "test-first" });
    expect(readFileSync(join(theirs, "SKILL.md"), "utf8")).toBe(own);
  });

  it("F-AG-13.1 Launch in terminal opens a pane named after the teammate whose first input starts with the brief", async () => {
    const before = await paneIds();
    const mark = fakeLog().length;
    await openTeammate(scout.id);
    await $('[data-testid="teammate-launch-terminal"]').click();
    const where = $(`//*[@role="menuitem"][normalize-space()="${basename(place)}"]`);
    await where.waitForDisplayed({ timeout: 10_000 });
    await where.click();
    let id = "";
    await browser.waitUntil(
      async () => {
        id = (await paneIds()).find((p) => !before.includes(p)) ?? "";
        return id !== "";
      },
      { timeout: 20_000, timeoutMsg: "no pane opened in Code mode" },
    );
    const tabs = (await invoke<Array<{ tabs: Array<{ id: string }> }>>("workspace_list")).flatMap((w) => w.tabs);
    const panes = (await Promise.all(tabs.map((t) => invoke<Pane[]>("pane_list", { tabId: t.id })))).flat();
    const record = panes.find((p) => p.id === id);
    expect(record?.humanName).toBe("Scout");
    expect(record?.engineId).toBe("claude");
    expect(record?.cwd).toBe(place);
    await browser.waitUntil(async () => pastes(mark).length > 0, {
      timeout: 60_000,
      timeoutMsg: `first inputs: ${JSON.stringify(pastes(mark))}`,
    });
    expect(pastes(mark)[0]?.startsWith(BRIEF)).toBe(true);
  });
});
