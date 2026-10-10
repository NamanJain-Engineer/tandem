import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, paneIds } from "../support/app";
import { sendAndWait } from "../support/agents";
import {
  browserSignIn,
  catalog,
  connectGithub,
  connectionOf,
  heldSignInUrl,
  mockReset,
  mockRevoke,
  mockState,
  mockSwitch,
  openPlugins,
} from "../support/plugins";

async function startSignIn(): Promise<void> {
  await $('[data-testid="plugin-connect-github"]').click();
  await $('[data-testid="connect-sheet"]').waitForDisplayed({ timeout: 10_000 });
  await $('[data-testid="connect-start"]').click();
}

// E2E-M12-2 OAuth failures (F-PL-03 AC2–AC4): metadata pointing sign-in at
// 169.254.169.254 is refused; a callback that arrives after Cancel is
// ignored; a reconnect whose check fails keeps the working credential.
describe("E2E-M12-2 OAuth failures", () => {
  before(async function () {
    this.timeout(60_000);
    await mockReset();
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m12-2-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await openPlugins();
  });

  it("F-PL-03.2 a sign-in redirected to 169.254.169.254 is refused and names the step", async function () {
    this.timeout(60_000);
    await mockSwitch({ private_redirect: "1" });
    browserSignIn("follow");
    await startSignIn();
    const err = $('[data-testid="connect-error"]');
    await err.waitForDisplayed({ timeout: 20_000 });
    const text = await err.getText();
    expect(text).toMatch(/Couldn't connect: .+ failed\./);
    expect(text).toContain("169.254.169.254");
    expect((await mockState()).clients).toBe(0);
    expect(await connectionOf("github")).toBeUndefined();
    await $('[data-testid="connect-cancel"]').click();
    await mockSwitch({ private_redirect: "0" });
  });

  it("F-PL-03.4 a callback after Cancel is ignored", async function () {
    this.timeout(60_000);
    browserSignIn("hold");
    await startSignIn();
    const url = await heldSignInUrl();
    await $('[data-testid="connect-cancel"]').click();
    await $('[data-testid="connect-sheet"]').waitForExist({ reverse: true, timeout: 5_000 });
    // The user finishes in the browser anyway, too late.
    await fetch(url, { redirect: "follow" }).catch(() => undefined);
    await browser.pause(1_000);
    expect(await connectionOf("github")).toBeUndefined();
    expect((await mockState()).access).toEqual([]);
    expect((await catalog()).find((c) => c.pluginId === "github")?.state).toBe("available");
    browserSignIn("follow");
  });

  it("F-PL-03.3 a reconnect whose check fails leaves the old credential in place", async function () {
    this.timeout(120_000);
    const conn = await connectGithub();
    const [oldToken] = (await mockState()).access;
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude" },
    });
    await invoke("grants_set", {
      subject: { type: "thread", threadId: thread.id },
      connectionId: conn,
      granted: true,
    });

    // Reconnect from the plugin's page. The provider rejects the token the
    // new sign-in gets: the switch is on only while that sign-in finishes,
    // so the working connection never sees it.
    browserSignIn("hold");
    await $('[data-testid="plugin-card-github"]').$("button=GitHub").click();
    await $('[data-testid="plugin-detail"]').waitForDisplayed({ timeout: 10_000 });
    await $('[data-testid="plugin-detail"] button[aria-label="More"]').click();
    await $('[role="menuitem"]=Reconnect').click();
    await $('[data-testid="connect-start"]').click();
    const url = await heldSignInUrl();
    await mockSwitch({ upstream_rejects: "1" });
    await fetch(url, { redirect: "follow" }).catch(() => undefined);
    const err = $('[data-testid="connect-error"]');
    await err.waitForDisplayed({ timeout: 20_000 });
    await mockSwitch({ upstream_rejects: "0" });
    expect(await err.getProperty("textContent")).toContain("Couldn't connect: Check the connection failed.");
    await $('[data-testid="connect-cancel"]').click();
    browserSignIn("follow");

    // Only the old token can still work: revoke the one the failed attempt got.
    const tokens = (await mockState()).access;
    expect(tokens).toHaveLength(2);
    for (const t of tokens.filter((t) => t !== oldToken)) await mockRevoke(t);
    const after = await connectionOf("github");
    expect(after?.connectionId).toBe(conn);
    expect(after?.state).toBe("connected");
    const reply = await sendAndWait(
      thread.id,
      'mcp tandem__call {"tool":"github__get_me","args":{}}',
      60_000,
    );
    expect(reply.join(" ")).toMatch(/The tool said:[\s\S]*octo-shop/);
  });
});
