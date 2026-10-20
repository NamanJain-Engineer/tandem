import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { addFolder, checkAgain, freshOffline, mod, setProfile, userHome } from "../support/app";
import { clickRow, editorTabs, openFiles } from "../support/files";

// E2E-M18-CD23-1 Code intelligence (F-CD-23): a Python folder, with the
// fake language server on the login PATH as `pyright-langserver`. F12 on a
// name opens its definition in the other file; Shift+F12 lists the
// references and one opens; the status line counts the server's
// diagnostic; a Rust file says there's no server for it.

const UTIL = "def helper():\n    return 1\n";
const MAIN = "import util\n\nutil.helper()\nx = ERROR\n";

const status = async () => String(await $('[data-testid="intel-status"]').getProperty("textContent"));

/** The caret on line `line` (1-based), `col` characters in. */
async function caretAt(line: number, col: number): Promise<void> {
  const content = await $('[data-testid="code-editor"] .cm-content');
  await content.waitForDisplayed({ timeout: 10_000 });
  await content.click();
  await browser.keys([mod(), "Home"]);
  for (let i = 1; i < line; i++) await browser.keys("ArrowDown");
  await browser.keys("Home");
  for (let i = 0; i < col; i++) await browser.keys("ArrowRight");
}

async function activeTab(): Promise<string | null> {
  return browser.execute(
    () =>
      document.querySelector<HTMLElement>('[data-testid="editor-tab"][aria-selected="true"]')?.dataset.path ??
      null,
  );
}

describe("E2E-M18-CD23-1 code intelligence", function () {
  this.timeout(300_000);
  const folder = `/tmp/tandem-e2e-cd23-${Date.now()}`;

  before(async () => {
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, "util.py"), UTIL);
    writeFileSync(join(folder, "main.py"), MAIN);
    writeFileSync(join(folder, "lib.rs"), "fn main() {}\n");
    execFileSync("git", ["-C", folder, "init", "-q", "-b", "main"]);
    // The fake server answers as pyright; nothing else is on the PATH.
    const bin = join(userHome(), "fakelsp");
    mkdirSync(bin, { recursive: true });
    const link = join(bin, "pyright-langserver");
    rmSync(link, { force: true });
    symlinkSync(join(dirname(process.env.TANDEM_FAKE_ENGINE_BIN ?? ""), "fake-lsp"), link);
    const profile = readFileSync(join(userHome(), ".zprofile"), "utf8").trimEnd().split("\n");
    setProfile([...profile, `export PATH="${bin}:$PATH"`]);
    await freshOffline();
    await checkAgain();
    await addFolder(folder);
    await openFiles();
  });

  after(async () => {
    setProfile(null);
    await checkAgain();
  });

  it("F-CD-23.3 the status line counts the server's diagnostics", async () => {
    await clickRow("main.py");
    await browser.waitUntil(async () => (await status()) === "1 error", {
      timeout: 30_000,
      timeoutMsg: `the status line says ${await status()}`,
    });
    expect(await $('[data-testid="code-editor"] .cm-diag-error').isExisting()).toBe(true);
  });

  it("F-CD-23.1 F12 opens the definition in the other file, in a new tab", async () => {
    await caretAt(3, 6);
    await browser.keys("F12");
    await browser.waitUntil(async () => (await activeTab()) === "util.py", {
      timeout: 15_000,
      timeoutMsg: `the active tab is ${await activeTab()}`,
    });
    expect(await editorTabs()).toEqual(["main.py", "util.py"]);
  });

  it("F-CD-23.2 Shift+F12 lists the references, and one opens", async () => {
    await $('[data-testid="editor-tab"][data-path="main.py"]').click();
    await caretAt(3, 6);
    await browser.keys(["Shift", "F12"]);
    const title = $('[data-testid="references-title"]');
    await browser.waitUntil(
      async () => String(await title.getProperty("textContent")) === "2 references to helper",
      {
        timeout: 15_000,
        timeoutMsg: "no references listed",
      },
    );
    const rows = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="reference-row"]')).map(
        (r) => `${r.dataset.path}:${r.dataset.line}`,
      ),
    );
    expect(rows.sort()).toEqual(["main.py:3", "util.py:1"]);
    await $('[data-testid="reference-row"][data-path="util.py"]').click();
    await browser.waitUntil(async () => (await activeTab()) === "util.py", { timeout: 10_000 });
  });

  it("F-CD-23.4 a Rust file says there's no language server for it", async () => {
    await clickRow("lib.rs");
    await browser.waitUntil(async () => (await status()) === "No language server for Rust", {
      timeout: 15_000,
      timeoutMsg: `the status line says ${await status()}`,
    });
  });
});
