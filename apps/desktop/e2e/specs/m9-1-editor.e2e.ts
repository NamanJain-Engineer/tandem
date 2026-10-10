import { $, browser } from "@wdio/globals";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { addFolder, freshOffline, mod, relaunch, settled } from "../support/app";
import { expectNoSeriousA11yViolations, expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { clickRow, editorTabs, editorText, openFiles, treeRows, typeAtEnd, waitForRow } from "../support/files";

// E2E-M9-1 Editor (F-CD-19): an agent changes a file on disk, then the user
// saves an edit: the conflict prompt; auto-save lands within 1 s; a gitignored
// node_modules is hidden until "Show ignored"; a relaunch reopens the same tabs.
describe("E2E-M9-1 editor", () => {
  const root = `/tmp/tandem-e2e-m9-1-${Date.now()}`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(`${root}/src`, { recursive: true });
    mkdirSync(`${root}/node_modules/left-pad`, { recursive: true });
    writeFileSync(`${root}/.gitignore`, "node_modules/\n");
    writeFileSync(`${root}/README.md`, "# shop\n");
    writeFileSync(`${root}/src/main.rs`, "fn main() {}\n");
    writeFileSync(`${root}/node_modules/left-pad/index.js`, "module.exports = 1;\n");
    await freshOffline();
    await addFolder(root);
    await openFiles();
  });

  it("F-CD-19.3 a gitignored node_modules is hidden until Show ignored", async function () {
    this.timeout(60_000);
    await waitForRow("README.md");
    expect(await treeRows()).toEqual(["src", ".gitignore", "README.md"]);
    await $('[data-testid="files-show-ignored"]').click();
    await waitForRow("node_modules");
    expect(await $('[data-testid="file-row"][data-path="node_modules"]').getAttribute("aria-label")).toBe("node_modules, ignored");
    await $('[data-testid="files-show-ignored"]').click();
    await browser.waitUntil(async () => !(await treeRows()).includes("node_modules"), { timeout: 10_000 });
    await expectNoSeriousA11yViolations();
  });

  it("F-CD-19.2 auto-save writes within 1 s of the last keystroke", async function () {
    this.timeout(60_000);
    await clickRow("README.md");
    await browser.waitUntil(async () => (await editorText()).includes("# shop"), { timeout: 10_000 });
    await typeAtEnd("saved by itself");
    const typed = Date.now();
    let landed = 0;
    await browser.waitUntil(
      async () => {
        if (readFileSync(`${root}/README.md`, "utf8").includes("saved by itself")) landed = Date.now();
        return landed > 0;
      },
      { timeout: 5_000, interval: 25, timeoutMsg: "auto-save never wrote the file" },
    );
    expect(landed - typed).toBeLessThan(1_000);
  });

  it("F-CD-19.1 saving after an agent changed the file shows the conflict prompt, never a silent overwrite", async function () {
    this.timeout(60_000);
    await $('[data-testid="file-row"][data-path="src"]').click();
    await clickRow("src/main.rs");
    await browser.waitUntil(async () => (await editorText()).includes("fn main() {}"), { timeout: 10_000 });
    // The user edits the text they see; meanwhile an agent rewrites the file
    // on disk; then the user saves. (Written before the edit, the change
    // would reload the untouched file first, and the edit would land on the
    // agent's text with nothing to conflict.)
    await typeAtEnd("// mine");
    writeFileSync(`${root}/src/main.rs`, "fn main() { agent(); }\n");
    await browser.keys([mod(), "s"]);
    const dialog = await $('[data-testid="conflict-dialog"]');
    await dialog.waitForExist({ timeout: 10_000 });
    const title = await browser.execute(() => document.querySelector('[role="dialog"] h2')?.textContent ?? "");
    expect(title).toBe("File changed on disk");
    expect(readFileSync(`${root}/src/main.rs`, "utf8")).toBe("fn main() { agent(); }\n");
    // Compare shows both versions; Reload takes the agent's. A click while the
    // dialog zooms in can miss (D-52), which under full-suite load it did.
    await settled();
    await $("button=Compare").click();
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.querySelector('[data-testid="conflict-diff"]')?.textContent ?? "")).includes("agent"),
      { timeout: 10_000 },
    );
    await $("button=Reload").click();
    await browser.waitUntil(async () => (await editorText()).includes("agent();"), { timeout: 10_000 });
    expect(await $('[data-testid="conflict-dialog"]').isExisting()).toBe(false);
    expect(readFileSync(`${root}/src/main.rs`, "utf8")).toBe("fn main() { agent(); }\n");
  });

  it("F-CD-19.4 a relaunch reopens the same file tabs for the workspace", async function () {
    this.timeout(120_000);
    await browser.waitUntil(async () => (await editorTabs()).length === 2, { timeout: 10_000 });
    expect(await editorTabs()).toEqual(["README.md", "src/main.rs"]);
    // Tab state saves shortly after the last change.
    await browser.pause(800);
    await relaunch();
    await $('[data-testid="files-panel"]').waitForExist({ timeout: 30_000 });
    await browser.waitUntil(async () => (await editorTabs()).length === 2, {
      timeout: 20_000,
      timeoutMsg: `tabs after relaunch: ${JSON.stringify(await editorTabs())}`,
    });
    expect(await editorTabs()).toEqual(["README.md", "src/main.rs"]);
    // The expanded folder came back too.
    await waitForRow("src/main.rs");
    // The tree, the tabs and the editor together, in both themes.
    await expectNoSeriousA11yViolationsInBothThemes();
  });
});
