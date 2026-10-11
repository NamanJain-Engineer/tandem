import { $, browser } from "@wdio/globals";
import { existsSync, readFileSync } from "node:fs";
import { Key } from "webdriverio";
import { PhoneClient } from "@tandem/remote/client";
import { parsePairingFragment } from "@tandem/remote/protocol";
import { MemoryPairingStore } from "@tandem/remote/store";
import { addFolder, freshUser, invoke, mod, paneIds, paneText, split, waitForPaneText } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { testRoute, waitForEntitlement } from "../support/hosted";
import { E2E_KNOWN_SENTENCE } from "../support/phone";

// E2E-M16-4 Phone (F-RM-03, F-BE-11): a scripted phone, running the PWA's
// own client code in the test process, pairs from Settings › Remote over the
// real relay. A replayed frame is rejected; an allow without local arming
// is queued; a 65 KB frame closes the socket; after Revoke the next command
// is rejected. The relay's log never holds the plaintext (run.ts checks the
// whole run's log too).

async function until(check: () => Promise<boolean>, timeoutMsg: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(check, { timeout, interval: 200, timeoutMsg });
}

async function openRemote(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Remote"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="settings-page-remote"]').waitForDisplayed({ timeout: 10_000 });
}

async function closeSettings(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(Key.Escape);
  await $('[data-testid="settings-page-remote"]').waitForExist({ reverse: true, timeout: 5_000 });
}

/** Clicks the grant switch in the paired phone's row. */
async function grant(key: "chat" | "terminal" | "approvals" | "routines"): Promise<void> {
  const sw = $(`[data-testid="remote-grant-${key}"] [role="switch"]`);
  await sw.waitForClickable({ timeout: 5_000 });
  if ((await sw.getAttribute("aria-checked")) !== "true") await sw.click();
  await until(async () => (await sw.getAttribute("aria-checked")) === "true", `${key} grant didn't turn on`);
}

/** Opens a raw relay socket as the phone and reports how it closes. */
function rawPhoneSocket(relay: string, token: string): Promise<{ ws: WebSocket; closed: Promise<number> }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(relay, [`tandem-pair.${token}`]);
    ws.binaryType = "arraybuffer";
    const closed = new Promise<number>((r) => ws.addEventListener("close", (ev) => r(ev.code)));
    ws.addEventListener("open", () => resolve({ ws, closed }));
    ws.addEventListener("error", () => reject(new Error("raw socket failed")));
  });
}

describe("E2E-M16-4 Phone", () => {
  const rejected: string[] = [];
  const store = new MemoryPairingStore();
  const phone = new PhoneClient(store, { rejected: (r) => rejected.push(r) }, undefined, "E2E phone");
  let agent = "";

  before(async function () {
    this.timeout(150_000);
    const { email } = await freshUser("phone");
    await testRoute("plan", { email, plan: "pro", paid: true });
    await waitForEntitlement((e) => e.plan === "pro", "Pro");
    await addFolder(`/tmp/tandem-e2e-phone-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
  });

  after(() => phone.disconnect());

  it("F-RM-03 pairs from Settings › Remote; typed text crosses the relay sealed", async function () {
    this.timeout(90_000);
    await openRemote();
    expect(await $('[data-testid="remote-phone"]').getText()).toContain("No phones are paired");
    await $('[data-testid="remote-pair"]').click();
    const linkEl = $('[data-testid="remote-pair-link"]');
    await linkEl.waitForExist({ timeout: 15_000 });
    const link = String(await linkEl.getProperty("textContent"));
    expect(link).toMatch(/^http:\/\/[^/]+\/remote#p=/);
    const parsed = parsePairingFragment(new URL(link).hash);
    expect(parsed?.relay).toMatch(/^ws:\/\/127\.0\.0\.1:8787\/v1\/relay\/pr_/);
    await phone.pair(parsed!);
    await $('[data-testid="remote-paired"]').waitForDisplayed({ timeout: 15_000 });
    expect(await $('[data-testid="remote-paired"]').getText()).toBe("Paired with E2E phone.");
    await browser.keys(Key.Escape);
    await $('[data-testid="remote-device"]').waitForDisplayed({ timeout: 10_000 });
    await expectNoSeriousA11yViolationsInBothThemes();

    // Terminal control starts off; the desktop turns it on.
    const off = await phone.command("pane.type", { paneId: agent, text: "too early" });
    expect(off).toMatchObject({ ok: false, code: "forbidden" });
    await grant("terminal");
    const snap = await phone.command<{ agents: Array<{ paneId: string | null }> }>("view.snapshot");
    expect(snap.ok && snap.data.agents.some((a) => a.paneId === agent)).toBe(true);
    const typed = await phone.command("pane.type", { paneId: agent, text: E2E_KNOWN_SENTENCE });
    expect(typed.ok).toBe(true);
    await waitForPaneText(agent, `you said: ${E2E_KNOWN_SENTENCE}`);

    const log = process.env.E2E_RELAY_LOG;
    if (log && existsSync(log)) {
      await until(async () => readFileSync(log, "utf8").includes("relay connect"), "the relay log has the connections");
      expect(readFileSync(log, "utf8")).not.toContain(E2E_KNOWN_SENTENCE);
    }
  });

  it("F-RM-03.2 a replayed command frame is rejected and runs once", async function () {
    this.timeout(60_000);
    const once = await phone.command("pane.type", { paneId: agent, text: "replay me once" });
    expect(once.ok).toBe(true);
    await waitForPaneText(agent, "you said: replay me once");
    const frame = phone.lastFrame;
    expect(frame).not.toBeNull();
    phone.sendRaw(frame!);
    await until(async () => rejected.includes("replay"), "the desktop didn't reject the replay");
    await browser.pause(1_000);
    const text = await paneText(agent);
    expect(text.split("you said: replay me once").length - 1).toBe(1);
  });

  it("F-RM-03.3 a phone approval without local arming is queued until the desktop confirms", async function () {
    this.timeout(60_000);
    await grant("approvals");
    const view = await invoke<{ id: string }>("test_fixture_agent", {
      agent: { id: "phone-ask", title: "deploy check", status: "waiting", engineId: null, workspaceId: null, preview: null, approval: "pnpm deploy" },
    });
    const res = await phone.command<{ state: string }>("approval.decide", { approvalId: view.id, decision: "allow" });
    expect(res).toEqual({ ok: true, data: { state: "queued" } });
    await browser.pause(500);
    const pending = await invoke<Array<{ id: string }>>("approvals_pending");
    expect(pending.some((a) => a.id === view.id)).toBe(true);
    const item = $('[data-testid="remote-queue-item"]');
    await item.waitForDisplayed({ timeout: 10_000 });
    expect(await item.getText()).toContain("E2E phone asked to allow: Run `pnpm deploy`");
    await item.$("button=Allow").click();
    await until(
      async () => !(await invoke<Array<{ id: string }>>("approvals_pending")).some((a) => a.id === view.id),
      "the confirmed allow didn't apply",
    );
  });

  it("F-BE-11 a 65 KB frame closes the socket with 1009", async function () {
    this.timeout(30_000);
    const p = phone.paired!;
    const raw = await rawPhoneSocket(p.link.relay, p.link.ptok);
    raw.ws.send(new Uint8Array(65 * 1024));
    expect(await raw.closed).toBe(1009);
    // The raw socket took the phone's place; the client comes back.
    await phone.resume();
    await until(async () => phone.connection === "online", "the phone didn't reconnect");
  });

  it("F-RM-03.4 after Revoke the next phone command is rejected", async function () {
    this.timeout(60_000);
    const relay = phone.paired!.link.relay;
    const token = phone.paired!.link.ptok;
    await $('[data-testid="remote-revoke"]').click();
    const confirm = $('//*[@role="alertdialog"]//button[normalize-space()="Revoke"]');
    await confirm.waitForClickable({ timeout: 5_000 });
    await confirm.click();
    await until(async () => phone.connection === "revoked", `the phone wasn't told: ${phone.connection}`);
    expect(await store.load()).toBeNull();
    const after = await phone.command("view.snapshot");
    expect(after.ok).toBe(false);
    const raw = await rawPhoneSocket(relay, token);
    expect(await raw.closed).toBe(1008);
    await until(
      async () => (await $('[data-testid="remote-phone"]').getText()).includes("No phones are paired"),
      "the revoked phone is still listed",
    );
    expect(await invoke<unknown[]>("phone_pairings_list")).toEqual([]);
    await closeSettings();
  });
});
