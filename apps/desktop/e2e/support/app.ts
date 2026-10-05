import { $, $$, browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const API = "http://127.0.0.1:8787";
export const MAILPIT = "http://localhost:8025";
export const PASSWORD = "Tandem-dev-pass-1";

export function home(): string {
  const h = process.env.TANDEM_HOME;
  if (!h) throw new Error("TANDEM_HOME isn't set; run through e2e/run.ts");
  return h;
}

/** Calls a Tauri command from the webview (debug builds, TANDEM_E2E=1). */
export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = (await browser.executeAsync(
    (c: string, a: Record<string, unknown>, done: (r: { ok?: unknown; err?: unknown }) => void) => {
      const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> } };
      w.__TAURI_INTERNALS__
        .invoke(c, a)
        .then((ok) => done({ ok }))
        .catch((err) => done({ err }));
    },
    cmd,
    args,
  )) as { ok?: T; err?: unknown };
  if ("err" in result && result.err !== undefined) throw new Error(`${cmd}: ${JSON.stringify(result.err)}`);
  return result.ok as T;
}

/** A tiny cookie jar for API calls from the test process. */
class Jar {
  private cookies = new Map<string, string>();
  store(res: Response) {
    for (const c of res.headers.getSetCookie()) {
      const [pair] = c.split(";");
      const i = pair?.indexOf("=") ?? -1;
      if (pair && i > 0) this.cookies.set(pair.slice(0, i).trim(), pair.slice(i + 1));
    }
  }
  header(): string {
    return [...this.cookies].map(([k, v]) => `${k}=${v}`).join("; ");
  }
  get(name: string): string | undefined {
    return this.cookies.get(name);
  }
}

async function call(jar: Jar, method: string, path: string, body?: unknown): Promise<Response> {
  const res = await fetch(`${API}${path}`, {
    method,
    redirect: "manual",
    headers: {
      cookie: jar.header(),
      ...(body !== undefined ? { "content-type": "application/json" } : {}),
      ...(jar.get("tandem_csrf") ? { "x-csrf-token": jar.get("tandem_csrf") ?? "" } : {}),
      origin: "http://localhost:3000",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  jar.store(res);
  return res;
}

/** Signs up on the API and verifies through mailpit; returns a signed-in jar. */
export async function signUpVerified(email: string, password = PASSWORD): Promise<Jar> {
  const jar = new Jar();
  await call(jar, "GET", "/v1/health");
  const signup = await call(jar, "POST", "/v1/auth/sign-up/email", { email, password, name: "E2E User" });
  if (!signup.ok) throw new Error(`sign-up failed: ${signup.status} ${await signup.text()}`);
  let link: string | undefined;
  await browser.waitUntil(
    async () => {
      const list = (await (await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:${email}`)}&limit=1`)).json()) as {
        messages?: Array<{ ID: string }>;
      };
      const id = list.messages?.[0]?.ID;
      if (!id) return false;
      const msg = (await (await fetch(`${MAILPIT}/api/v1/message/${id}`)).json()) as { Text: string };
      link = msg.Text.match(/https?:\/\/\S+/)?.[0];
      return !!link;
    },
    { timeout: 20_000, timeoutMsg: "no verification mail" },
  );
  const url = new URL(link ?? "");
  await call(jar, "GET", url.pathname + url.search);
  const signin = await call(jar, "POST", "/v1/auth/sign-in/email", { email, password });
  if (!signin.ok) throw new Error(`sign-in failed: ${signin.status}`);
  return jar;
}

/** Desktop sessions of the jar's user (`/devices`), newest first. */
export async function deviceSessions(jar: Jar): Promise<Array<{ id: string }>> {
  const res = await call(jar, "GET", "/v1/me/sessions");
  const body = (await res.json()) as { sessions?: Array<{ id: string }> } | Array<{ id: string }>;
  return Array.isArray(body) ? body : (body.sessions ?? []);
}

export async function revokeDevice(jar: Jar, id: string): Promise<void> {
  const res = await call(jar, "DELETE", `/v1/me/sessions/${id}`);
  if (res.status !== 204) throw new Error(`revoke failed: ${res.status}`);
}

/** Who the scripted browser signs in as (read by browser-signin.ts). */
export function browserAs(email: string, password = PASSWORD, checkout: "complete" | "cancel" = "complete"): void {
  writeFileSync(join(home(), "e2e-browser.json"), JSON.stringify({ email, password, checkout }));
}

export function browserLog(): string {
  const f = join(home(), "e2e-browser.log");
  return existsSync(f) ? readFileSync(f, "utf8") : "";
}

/** The folder the fake picker returns next (TANDEM_FAKE_PICK_FOLDER). */
export function pickFolder(path: string): string {
  mkdirSync(path, { recursive: true });
  writeFileSync(join(home(), "pick-folder.txt"), path);
  return path;
}

export function uniqueEmail(tag: string): string {
  return `e2e-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}@tandem.test`;
}

/** Waits for the app shell or the sign-in screen after a (re)launch. */
export async function waitForApp(): Promise<void> {
  await browser.waitUntil(async () => (await browser.getTitle()) === "Tandem", { timeout: 30_000 });
  await browser.waitUntil(async () => (await $("header[role=banner]").isExisting()) || (await $("#signin-title").isExisting()), {
    timeout: 30_000,
    timeoutMsg: "neither the shell nor sign-in appeared",
  });
}

/** Clicks sign-in and waits for the shell (the scripted browser signs in). */
export async function signInDesktop(email: string): Promise<void> {
  browserAs(email);
  await $("button=Continue with email").click();
  await $('[data-testid="plan-footer"]').waitForExist({ timeout: 60_000, timeoutMsg: `sign-in didn't finish:\n${browserLog()}` });
}

/** `innerText` of the first match. WebKitWebDriver's getText drops text
 * styled with text-overflow/uppercase, so read it in the page instead. */
export async function textOf(selector: string): Promise<string> {
  const text = await browser.execute((sel: string) => (document.querySelector(sel) as HTMLElement | null)?.innerText ?? "", selector);
  return String(text).trim();
}

export async function planFooter(): Promise<string> {
  return textOf('[data-testid="plan-footer"]');
}

/** Adds a folder through the rail "+" and the fake picker. */
export async function addFolder(path: string): Promise<void> {
  pickFolder(path);
  await $('nav[aria-label="Workspaces"] button[aria-label="Add folder…"]').click();
}

export async function workspaceRoots(): Promise<string[]> {
  const rows = await $$('[data-testid="workspace-row"]');
  const roots: string[] = [];
  for (const r of rows) roots.push((await r.getAttribute("data-root")) ?? "");
  return roots;
}

/** Text of the visible terminal. */
export async function terminalText(): Promise<string> {
  const text = await browser.execute(() => {
    for (const el of Array.from(document.querySelectorAll('[data-testid="terminal"]'))) {
      const host = el as HTMLElement;
      if (host.offsetParent !== null) return (host.querySelector(".xterm-rows") as HTMLElement | null)?.innerText ?? "";
    }
    return "";
  });
  return String(text);
}

/** Types into the visible terminal. */
export async function typeInTerminal(text: string): Promise<void> {
  const terms = await $$('[data-testid="terminal"]');
  for (const t of terms) {
    if (await t.isDisplayed()) {
      await t.click();
      // One action per key: WebKitWebDriver merges repeated keys in one action.
      for (const ch of text) await browser.keys(ch === "\n" ? "Enter" : ch);
      return;
    }
  }
  throw new Error("no visible terminal");
}

/** Lines of the visible terminal, trimmed. */
export async function terminalLines(): Promise<string[]> {
  return (await terminalText()).split("\n").map((l) => l.trim());
}

/** Restarts the app (same TANDEM_HOME), optionally with launch arguments. */
export async function relaunch(args: string[] = []): Promise<void> {
  await browser.reloadSession({
    "tauri:options": { application: process.env.TANDEM_APP_BINARY, args },
    "wdio:enforceWebDriverClassic": true,
  } as unknown as WebdriverIO.Capabilities);
  await waitForApp();
}

export function mod(): string {
  return process.platform === "darwin" ? "Meta" : "Control";
}

/** Clears the shared TANDEM_HOME's account and workspaces, whichever screen
 * is showing. The E2E keychain lives in memory, so a relaunch always comes
 * back signed out; specs that relaunch use offline mode instead. */
async function resetState(): Promise<void> {
  await waitForApp();
  const list = await invoke<Array<{ id: string }>>("workspace_list");
  for (const w of list) await invoke("workspace_remove", { id: w.id });
  await invoke("auth_sign_out");
  await $("#signin-title").waitForDisplayed({ timeout: 20_000 });
}

/** A new verified account, signed in on the desktop, no workspaces, Free. */
export async function freshUser(tag: string): Promise<{ email: string; jar: Awaited<ReturnType<typeof signUpVerified>> }> {
  const email = uniqueEmail(tag);
  const jar = await signUpVerified(email);
  await resetState();
  await signInDesktop(email);
  return { email, jar };
}

/** "Use Tandem offline" with no workspaces: survives relaunches. */
export async function freshOffline(): Promise<void> {
  await resetState();
  await $("button=Use Tandem offline").click();
  await $('[data-testid="plan-footer"]').waitForExist({ timeout: 20_000 });
}

/** Dismisses whatever toast or dialog is open with Escape. */
export async function escape(): Promise<void> {
  await browser.keys("Escape");
}

/** Types into the focused element, one key per action. */
export async function typeKeys(text: string): Promise<void> {
  for (const ch of text) await browser.keys(ch);
}

/** Writes to the visible terminal's PTY directly (the path xterm's input
 * uses), for long or escape-heavy input the driver can't type reliably. */
export async function sendToTerminal(text: string): Promise<void> {
  const paneId = await browser.execute(() => {
    for (const el of Array.from(document.querySelectorAll('[data-testid="terminal"]'))) {
      const host = el as HTMLElement;
      if (host.offsetParent !== null) return host.dataset.paneId ?? null;
    }
    return null;
  });
  if (!paneId) throw new Error("no visible terminal");
  await invoke("pty_write", { paneId, data: text });
}
