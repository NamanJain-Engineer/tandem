import { $, browser } from "@wdio/globals";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  closeAllPanes,
  freshOffline,
  home,
  invoke,
  paneIds,
  paneText,
  sendToPane,
  split,
  waitForPaneText,
} from "../support/app";

// E2E-M18-TL12B `vibezeo run` for agents (F-TL-12 slice 2), with the fake
// keychain and the fake engine's `/sh`: the agent asks the app to run a
// command with the folder's secrets; the pane asks first, naming the command
// and the secret; the output comes back with the value masked, and the
// agent's own environment never has it.

const VALUE = "sk_test_e2e_agent_4242";

/** Waits for `/sh` after `mark` to report its exit code. */
async function exitAfter(pane: string, mark: string): Promise<number> {
  let code = -2;
  await browser.waitUntil(
    async () => {
      const text = await paneText(pane);
      const at = text.lastIndexOf(mark);
      const m = at >= 0 ? /sh exit: (-?\d+)/.exec(text.slice(at)) : null;
      if (m) code = Number(m[1]);
      return m !== null;
    },
    { timeout: 20_000, timeoutMsg: `no exit code after ${mark}` },
  );
  return code;
}

async function textAfter(pane: string, mark: string): Promise<string> {
  const text = await paneText(pane);
  return text.slice(text.lastIndexOf(mark));
}

describe("E2E-M18-TL12B vibezeo run for agents", function () {
  this.timeout(180_000);
  const folder = `/tmp/tandem-e2e-m18-tl12b-${Date.now()}`;
  let agent = "";
  const banner = () => `[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-approval"]`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, ".env"), "API_KEY=vibezeo-secret://stripe-test\n");
    await freshOffline();
    await invoke("secrets_set", { name: "stripe-test", value: VALUE });
    await addFolder(folder);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
  });

  after(async () => {
    await closeAllPanes();
    for (const s of await invoke<Array<{ name: string }>>("secrets_list"))
      await invoke("secrets_delete", { name: s.name });
    rmSync(folder, { recursive: true, force: true });
  });

  it("F-TL-12.7 the pane asks first by name; Allow once runs it and the value comes back masked", async () => {
    await sendToPane(agent, "/sh echo R1; vibezeo run -- printenv API_KEY\r");
    await $(banner()).waitForDisplayed({ timeout: 15_000 });
    const summary = String(await $(`${banner()} [data-testid="run-line"]`).getProperty("textContent"));
    expect(summary).toContain("printenv API_KEY");
    expect(summary).toContain("API_KEY (stripe-test)");
    expect(summary).not.toContain(VALUE);
    await $(`${banner()} [data-testid="pane-approval-once"]`).click();
    expect(await exitAfter(agent, "R1")).toBe(0);
    expect(await textAfter(agent, "R1")).toContain("[REDACTED]");
    expect(await paneText(agent)).not.toContain(VALUE);
    await $(banner()).waitForExist({ reverse: true, timeout: 5_000 });
    // The agent's own environment never had it.
    await sendToPane(agent, '/sh echo R2; echo "own=${API_KEY:-none}"\r');
    expect(await exitAfter(agent, "R2")).toBe(0);
    expect(await textAfter(agent, "R2")).toContain("own=none");
  });

  it("F-TL-12.9 Deny runs nothing; Allow for this agent lets the same command run again without asking", async () => {
    await sendToPane(agent, "/sh echo R3; vibezeo run -- touch ran\r");
    await $(banner()).waitForDisplayed({ timeout: 15_000 });
    await $(`${banner()} [data-testid="pane-approval-deny"]`).click();
    expect(await exitAfter(agent, "R3")).toBe(1);
    expect(await textAfter(agent, "R3")).toContain("Not allowed: you declined");
    expect(existsSync(join(folder, "ran"))).toBe(false);

    await sendToPane(agent, "/sh echo R4; vibezeo run -- echo agent-ok\r");
    await $(banner()).waitForDisplayed({ timeout: 15_000 });
    await $(`${banner()} [data-testid="pane-approval-agent"]`).click();
    expect(await exitAfter(agent, "R4")).toBe(0);
    await sendToPane(agent, "/sh echo R5; vibezeo run -- echo agent-ok\r");
    expect(await exitAfter(agent, "R5")).toBe(0);
    expect(await textAfter(agent, "R5")).toContain("agent-ok");
    expect(await $(banner()).isExisting()).toBe(false);
  });

  it("F-TL-12.10 outside an agent pane it says why and exits 1", async () => {
    const plain = (await paneIds()).find((p) => p !== agent) ?? "";
    const vibezeo = join(home(), "run", "bin", "vibezeo");
    await sendToPane(plain, `'${vibezeo}' run -- echo hi; echo "code=$?"\r`);
    await waitForPaneText(plain, "code=1");
    expect(await paneText(plain)).toContain("vibezeo run works in agent panes in Vibezeo");
  });
});
