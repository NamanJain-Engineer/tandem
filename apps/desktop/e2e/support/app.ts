import { $, $$, browser } from "@wdio/globals";
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

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
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> };
      };
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
export class Jar {
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

export async function call(jar: Jar, method: string, path: string, body?: unknown): Promise<Response> {
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
      const list = (await (
        await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:${email}`)}&limit=1`)
      ).json()) as {
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

/** Puts the user on `plan` through the API's test route (no checkout); the
 * desktop follows over its entitlement stream. */
export async function setPlan(email: string, plan: "free" | "pro" | "team"): Promise<void> {
  const res = await fetch(`${API}/__test/plan`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, plan }),
  });
  if (!res.ok) throw new Error(`setting the plan failed: ${res.status} ${await res.text()}`);
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
export function browserAs(
  email: string,
  password = PASSWORD,
  checkout: "complete" | "cancel" = "complete",
): void {
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
  await browser.waitUntil(async () => (await browser.getTitle()) === "Vibezeo", { timeout: 30_000 });
  await browser.waitUntil(
    async () =>
      (await $("header[role=banner]").isExisting()) ||
      (await $("#signin-title").isExisting()) ||
      (await $('[data-testid="onboarding"]').isExisting()),
    { timeout: 30_000, timeoutMsg: "neither the shell, sign-in nor the setup wizard appeared" },
  );
}

/** A fresh profile shows the setup wizard after sign-in: skip every step. */
export async function skipOnboarding(): Promise<void> {
  for (let i = 0; i < 6 && (await $('[data-testid="onboarding"]').isExisting()); i++) {
    await $('[data-testid="onboarding-skip"]').click();
    await browser.pause(150);
  }
}

/** Clicks sign-in and waits for the shell (the scripted browser signs in). */
export async function signInDesktop(email: string): Promise<void> {
  browserAs(email);
  await $("button=Continue with email").click();
  await browser.waitUntil(
    async () =>
      (await $('[data-testid="app-shell"]').isExisting()) ||
      (await $('[data-testid="onboarding"]').isExisting()),
    { timeout: 60_000, timeoutMsg: `sign-in didn't finish:\n${browserLog()}` },
  );
  // A brand-new profile (the first spec of a run) gets the wizard first.
  await skipOnboarding();
  await $('[data-testid="app-shell"]').waitForExist({ timeout: 20_000 });
}

/** `innerText` of the first match. WebKitWebDriver's getText drops text
 * styled with text-overflow/uppercase, so read it in the page instead. */
export async function textOf(selector: string): Promise<string> {
  const text = await browser.execute(
    (sel: string) => (document.querySelector(sel) as HTMLElement | null)?.innerText ?? "",
    selector,
  );
  return String(text).trim();
}

/** Waits until the desktop is signed in to its Vibezeo account (the rail's
 * plan footer used to show it; there's no plan UI since D-869). */
export async function waitSignedIn(timeout = 20_000): Promise<void> {
  await browser.waitUntil(async () => (await invoke<{ signedIn: boolean }>("auth_state")).signedIn, {
    timeout,
    timeoutMsg: "the desktop never signed in",
  });
}

/** Waits until `ok` holds for what `read` gives, and returns that. A timeout
 * says what `read` last gave: a `timeoutMsg` is built before the wait starts,
 * so one that reads the state shows it as it was then. */
export async function waitFor<T>(
  read: () => T | Promise<T>,
  ok: (value: T) => boolean,
  what: string,
  options: { timeout: number; interval?: number },
): Promise<T> {
  let last: T | undefined;
  try {
    await browser.waitUntil(async () => {
      last = await read();
      return ok(last);
    }, options);
  } catch (e) {
    throw new Error(`${what}: ${JSON.stringify(last)} (${String(e)})`);
  }
  return last as T;
}

/** Adds a folder through the rail "+" and the fake picker. */
export async function addFolder(path: string, { added = true }: { added?: boolean } = {}): Promise<void> {
  pickFolder(path);
  await $('nav[aria-label="Workspaces"] button[aria-label="Add folder…"]').click();
  // A refused add (a plan limit) never shows up; the caller checks why.
  if (!added) return;
  // The fake picker reads its file asynchronously: wait for this folder
  // before another call can rewrite it.
  await waitFor(workspaceRoots, (roots) => roots.includes(path), `${path} wasn't added`, { timeout: 15_000 });
}

/** Rail rows' roots, read in the page (rows re-render while a list refreshes). */
export async function workspaceRoots(): Promise<string[]> {
  const roots = await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="workspace-row"]')).map(
      (r) => r.dataset.root ?? "",
    ),
  );
  return roots as string[];
}

/** Text of the visible terminal, from xterm's buffer (WebGL panes draw to a
 * canvas, so the DOM has no rows to read). */
export async function terminalText(): Promise<string> {
  const text = await browser.execute(() => {
    const test = (window as unknown as { __tandemTest?: { paneText: (id: string) => string } }).__tandemTest;
    for (const el of Array.from(document.querySelectorAll('[data-testid="terminal"]'))) {
      const host = el as HTMLElement;
      if (host.offsetParent !== null) return test?.paneText(host.dataset.paneId ?? "") ?? "";
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

/** Stops the runs still going and archives every task (F-TK-04, F-TK-06):
 * tasks one spec made never show on the next one's board. */
async function clearTasks(): Promise<void> {
  const going = new Set(["queued", "setup", "running", "reviewing"]);
  for (const a of await invoke<Array<{ attempt: { id: string; state: string } }>>("task_attempts_list")) {
    if (going.has(a.attempt.state))
      await invoke("task_attempt_stop", { attemptId: a.attempt.id }).catch(() => undefined);
  }
  // A page at a time, until none is left. A task that changed since it
  // was listed is archived on the next pass, at its new revision.
  for (let page = 0; page < 40; page++) {
    const { items } = await invoke<{ items: Array<{ id: string; revision: number }> }>("tasks_list", {
      filter: {},
      cursor: null,
    });
    if (items.length === 0) return;
    for (const i of items)
      await invoke("task_archive", { id: i.id, expectedRevision: i.revision }).catch(() => undefined);
  }
  throw new Error("resetState: tasks were still there after 40 passes of archiving");
}

/** Clears the shared TANDEM_HOME's account and workspaces, whichever screen
 * is showing. The E2E keychain lives in memory, so a relaunch always comes
 * back signed out; specs that relaunch use offline mode instead. */
async function resetState({ onboarded = true }: { onboarded?: boolean } = {}): Promise<void> {
  await waitForApp();
  const list = await invoke<Array<{ id: string; parentId: string | null }>>("workspace_list");
  // Removing a repository removes its worktrees with it (F-CD-15).
  for (const w of list.filter((w) => !w.parentId)) await invoke("workspace_remove", { id: w.id });
  // Threads outlive specs too: archive, then delete each (F-TH-01).
  for (const th of await invoke<Array<{ id: string }>>("thread_list", {
    kind: "thread",
    includeArchived: true,
  })) {
    await invoke("thread_archive", { threadId: th.id, archived: true });
    await invoke("thread_delete", { threadId: th.id });
  }
  // Command history outlives specs too (F-EX-02): a command one spec's
  // shell ran must not show in another's recall.
  await invoke("history_clear");
  // Plans outlive specs too since they're kept (F-TK-07); their tasks are
  // the tracker's, which specs share.
  for (const p of await invoke<Array<{ id: string }>>("plan_list"))
    await invoke("plan_discard", { id: p.id });
  // So is the tracker: a run left going stops, and every task is archived,
  // so a spec starts with no tasks in sight (archived ones keep their
  // numbers). Every spec that uses tasks resets once, before its own.
  await clearTasks();
  // Routines outlive specs too (M11): they go, with the background runner
  // and the global pause.
  for (const v of await invoke<Array<{ routine: { id: string } }>>("routines_list")) {
    await invoke("routine_delete", { id: v.routine.id });
  }
  await invoke("routines_background_set", { enabled: false });
  await invoke("settings_set", { key: "pause_all_routines", value: false });
  // Teammates outlive specs too (M10): their chats go, every teammate but the
  // Lead goes, and the Lead and mail are back to their defaults.
  for (const s of await invoke<Array<{ teammate: { id: string; isLead: boolean } }>>("teammate_list")) {
    const chats = await invoke<Array<{ id: string }>>("thread_list", {
      kind: "teammate_chat",
      teammateId: s.teammate.id,
      includeArchived: true,
    });
    for (const c of chats) {
      await invoke("thread_archive", { threadId: c.id, archived: true });
      await invoke("thread_delete", { threadId: c.id });
    }
    if (!s.teammate.isLead) await invoke("teammate_delete", { id: s.teammate.id });
    else
      await invoke("teammate_update", {
        id: s.teammate.id,
        patch: {
          pinned: true,
          settings: {
            memoryCap: 4096,
            learnMode: "ask",
            mayMessage: true,
            maySchedule: false,
            defaultAccess: "edits",
          },
        },
      });
  }
  await invoke("settings_set", { key: "pause_all_mail", value: false });
  // Plugins outlive specs too (M12): every connection goes, and its grants
  // and held drafts with it; no Team allowlist fixture.
  for (const c of await invoke<Array<{ connectionId: string | null }>>("plugins_catalog")) {
    if (c.connectionId) await invoke("plugin_disconnect", { connectionId: c.connectionId });
  }
  await invoke("test_plugin_allowlist", { pluginIds: null });
  // Added ACP agents outlive specs too (F-EN-10): a later spec's engine
  // lists and launch grid must not show one.
  for (const a of await invoke<Array<{ id: string }>>("acp_agents_list")) {
    await invoke("acp_agent_remove", { id: a.id });
  }
  await invoke("auth_sign_out");
  // A first run has no engine accounts yet (the wizard's Accounts step, D-114).
  if (!onboarded) {
    for (const a of await invoke<Array<{ id: string }>>("accounts_list"))
      await invoke("accounts_remove", { accountId: a.id });
  }
  // Specs start past the first-run wizard unless they test it (E2E-M5-9).
  await invoke("onboarding_set", onboarded ? { done: true } : { step: "engines" });
  // UI state that outlives a spec: start in Code mode with the sidebar closed.
  await invoke("settings_set", { key: "last_mode", value: "code" });
  // New threads start from the app's defaults, not a spec's last engine (F-TH-07).
  await invoke("settings_set", { key: "thread_defaults", value: {} });
  // Every spec starts with the default shortcuts (F-SH-09 rebinding).
  await invoke("settings_set", { key: "shortcut_overrides", value: {} });
  await invoke("sidebar_save", {
    sidebar: {
      open: false,
      tabs: [{ id: "dashboard", kind: "dashboard", title: null, url: null }],
      active: "dashboard",
      width: 470,
      fullscreen: false,
      pinned: false,
    },
  });
  // The removals bypassed the UI's stores (the launch already showed the last
  // spec's workspace): reload so the UI starts from what the backend has.
  await browser.refresh();
  await waitForApp();
  await $("#signin-title").waitForDisplayed({ timeout: 20_000 });
}

/** Back to the welcome screen with no workspaces, past the wizard. */
export async function resetToWelcome(): Promise<void> {
  await resetState();
}

/** A new verified account, signed in on the desktop, no workspaces, Free. */
export async function freshUser(
  tag: string,
): Promise<{ email: string; jar: Awaited<ReturnType<typeof signUpVerified>> }> {
  const email = uniqueEmail(tag);
  const jar = await signUpVerified(email);
  await resetState();
  await signInDesktop(email);
  return { email, jar };
}

/** Signed out at the sign-in screen, past the wizard, with no workspaces:
 * whatever an earlier spec file left (files run in name order). */
export async function freshSignedOut(): Promise<void> {
  await resetState();
}

/** A profile as on first launch: signed out, no workspaces, the wizard not run. */
export async function freshProfile(): Promise<void> {
  await resetState({ onboarded: false });
}

/** "Use Vibezeo offline" with no workspaces: survives relaunches. */
export async function freshOffline(): Promise<void> {
  await resetState();
  await $("button=Use Vibezeo offline").click();
  await $('[data-testid="app-shell"]').waitForExist({ timeout: 20_000 });
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

/** Ids of the visible panes, in layout (DOM) order. */
export async function paneIds(): Promise<string[]> {
  const ids = await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="pane"]'))
      .filter((el) => el.offsetParent !== null)
      .map((el) => el.dataset.paneId ?? ""),
  );
  return ids as string[];
}

/** Screen text of one pane's terminal. */
export async function paneText(paneId: string): Promise<string> {
  const text = await browser.execute(
    (id: string) =>
      (window as unknown as { __tandemTest?: { paneText: (id: string) => string } }).__tandemTest?.paneText(
        id,
      ) ?? "",
    paneId,
  );
  return String(text);
}

/** Writes to one pane's PTY. */
export async function sendToPane(paneId: string, text: string): Promise<void> {
  await invoke("pty_write", { paneId, data: text });
}

/** Waits until a pane's screen shows `needle`. With `commandLine`, the
 * needle is looked for in the shell's command line, which zsh breaks with
 * real newlines at the pane's width (program output wraps softly and is
 * joined by `paneText` already). */
export async function waitForPaneText(
  paneId: string,
  needle: string,
  timeout = 15_000,
  opts: { commandLine?: boolean } = {},
): Promise<void> {
  const text = async () => {
    const t = await paneText(paneId);
    return opts.commandLine ? t.replace(/\n/g, "") : t;
  };
  await waitFor(text, (t) => t.includes(needle), `pane ${paneId} never showed ${JSON.stringify(needle)}`, {
    timeout,
  });
}

/** A shell's pid, read through the shell itself. */
export async function shellPid(paneId: string): Promise<number> {
  const tag = `pid-${Math.random().toString(36).slice(2, 8)}`;
  await sendToPane(paneId, `echo ${tag}=$$\n`);
  await waitForPaneText(paneId, `${tag}=`);
  const match = new RegExp(`${tag}=(\\d+)`).exec(await paneText(paneId));
  if (!match?.[1]) throw new Error(`no pid in pane ${paneId}`);
  return Number(match[1]);
}

/** Waits until a pane's process has printed something (its prompt). */
export async function waitForShell(paneId: string, timeout = 20_000): Promise<void> {
  if (!paneId) throw new Error("waitForShell: no pane id (the pane wasn't visible)");
  await browser.waitUntil(async () => (await paneText(paneId)).trim().length > 0, {
    timeout,
    timeoutMsg: `pane ${paneId} never printed anything`,
  });
}

/** Waits until finite CSS animations (a dialog zooming in) have finished,
 * so a click lands where the element ends up. */
export async function settled(): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute(() =>
        document
          .getAnimations()
          .every((a) => a.playState !== "running" || a.effect?.getTiming().iterations === Infinity),
      ),
    { timeout: 5_000, timeoutMsg: "animations still running" },
  );
}

/** A pointer or key event the page received, as `recordInput()` saw it. */
export type InputRecord = {
  type: string;
  target: string;
  inDialog: boolean;
  x: number;
  y: number;
  /** The watched element's box when the event arrived: [x, y, width, height]. */
  watched: number[] | null;
  at: number;
};

/** Starts recording the page's pointer and key events (capture phase, before
 * the app or a dialog sees them) with the box of `selector` at that moment,
 * and returns the page's clock to read them from. */
export async function recordInput(selector: string): Promise<number> {
  return (await browser.execute((sel: string) => {
    type Rec = {
      type: string;
      target: string;
      inDialog: boolean;
      x: number;
      y: number;
      watched: number[] | null;
      at: number;
    };
    const w = window as unknown as { __e2eInput?: Rec[]; __e2eWatch?: string };
    w.__e2eWatch = sel;
    if (!w.__e2eInput) {
      const log: Rec[] = [];
      w.__e2eInput = log;
      const rec = (e: Event) => {
        const el = e.target instanceof Element ? e.target : null;
        const watched = document.querySelector(w.__e2eWatch ?? "")?.getBoundingClientRect();
        const pointer = e as PointerEvent;
        log.push({
          type: e.type === "keydown" ? `keydown ${(e as KeyboardEvent).key}` : e.type,
          target: el
            ? `${el.tagName}${el.getAttribute("data-testid") ? `[${el.getAttribute("data-testid")}]` : ""}`
            : String(e.target),
          inDialog: !!el?.closest('[role="dialog"],[role="alertdialog"]'),
          x: Math.round(pointer.clientX ?? -1),
          y: Math.round(pointer.clientY ?? -1),
          watched: watched ? [watched.x, watched.y, watched.width, watched.height].map(Math.round) : null,
          at: Math.round(performance.now()),
        });
        if (log.length > 40) log.shift();
      };
      for (const type of ["pointerdown", "pointerup", "click", "keydown"])
        window.addEventListener(type, rec, true);
    }
    return Math.round(performance.now());
  }, selector)) as number;
}

export async function inputSince(since: number): Promise<InputRecord[]> {
  const all = (await browser.execute(
    () => (window as unknown as { __e2eInput?: unknown[] }).__e2eInput ?? [],
  )) as InputRecord[];
  return all.filter((r) => r.at >= since);
}

/** The click on a picker tile reached the page outside that tile, so the
 * picker took it as a click outside and closed without starting anything. */
export class PickMissed extends Error {}

/** Picks a tile in the open pane picker. Under load WebKitWebDriver has
 * dropped a click on a dialog that had just opened (D-52), in every spec that
 * clicked a picker tile directly (D-830). Tiles are disabled while a pick
 * runs, so one more click can only start a pick that never started. A click
 * the page received outside the tile's box closes the picker as any click
 * outside does; that throws `PickMissed`, with what the page received, so
 * the caller can open the picker again (D-834). */
export async function pickTile(tile: string): Promise<void> {
  const selector = `[data-testid="pane-picker"] [data-testid="tile-${tile}"]`;
  // With every engine installed, a tile can sit below the picker's fold.
  await $(selector).waitForExist({ timeout: 10_000 });
  await $(selector).scrollIntoView({ block: "nearest" });
  await $(selector).waitForClickable({ timeout: 10_000 });
  await settled();
  const since = await recordInput(selector);
  await $(selector).click();
  const closed = await browser
    .waitUntil(async () => !(await $('[data-testid="pane-picker"]').isExisting()), { timeout: 2_000 })
    .catch(() => false);
  if (
    !closed &&
    (await $(selector)
      .isEnabled()
      .catch(() => false))
  ) {
    console.warn(`pickTile: the ${tile} tile click didn't register; clicking again`);
    await $(selector).click();
    return;
  }
  if (!closed) return;
  const input = await inputSince(since);
  const down = input.find((r) => r.type === "pointerdown");
  const box = down?.watched;
  const onTile =
    !!down &&
    !!box &&
    down.x >= box[0]! &&
    down.x <= box[0]! + box[2]! &&
    down.y >= box[1]! &&
    down.y <= box[1]! + box[3]!;
  // Outside the tile's box: the click never reached it. On the box but not
  // in the dialog would be the app's own hit testing, so that isn't retried.
  if (down && !down.inDialog && !onTile)
    throw new PickMissed(
      `the ${tile} tile click reached the page outside the tile: ${JSON.stringify(input)}`,
    );
}

/** Splits the focused pane (Mod+D or Mod+Shift+D) and picks a tile. */
export async function split(direction: "right" | "down", tile = "shell"): Promise<string> {
  const before = await paneIds();
  const open = async () => {
    await browser.execute((dir: string) => {
      const mac = navigator.platform.toLowerCase().includes("mac");
      // Outside a terminal Ctrl+D splits right; Ctrl+Shift+D splits down everywhere.
      (document.activeElement as HTMLElement | null)?.blur();
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          code: "KeyD",
          key: "d",
          ctrlKey: !mac,
          metaKey: mac,
          shiftKey: dir === "down",
          bubbles: true,
          cancelable: true,
        }),
      );
    }, direction);
    await $('[data-testid="pane-picker"]').waitForExist({ timeout: 10_000 });
    // Where the picker opened, for the failure message below.
    return (await browser.execute(
      () =>
        (window as unknown as { __tandemTest?: { debug: () => { picker?: unknown } } }).__tandemTest?.debug()
          .picker,
    )) as { tabId?: string } | null | undefined;
  };
  let opened = await open();
  try {
    await pickTile(tile);
  } catch (e) {
    if (!(e instanceof PickMissed)) throw e;
    console.warn(`split: ${e.message}; opening the picker again`);
    opened = await open();
    await pickTile(tile);
  }
  let added = "";
  // Toasts expire before the timeout; keep any seen while waiting.
  const seen = new Set<string>();
  await browser
    .waitUntil(
      async () => {
        const toasts = (await browser.execute(() =>
          Array.from(document.querySelectorAll("[role=status], [role=alert]")).map((el) =>
            (el.textContent ?? "").trim(),
          ),
        )) as string[];
        for (const t of toasts) if (t) seen.add(t);
        const now = await paneIds();
        added = now.find((id) => !before.includes(id)) ?? "";
        return added !== "";
      },
      { timeout: 15_000, interval: 200 },
    )
    .catch(async () => {
      const state = await browser.execute(() =>
        (window as unknown as { __tandemTest?: { debug: () => unknown } }).__tandemTest?.debug(),
      );
      // Whether the core made the pane, in the tab the picker opened for.
      const inCore = opened?.tabId
        ? await invoke<Array<{ id: string }>>("pane_list", { tabId: opened.tabId })
            .then((panes) => panes.map((p) => p.id))
            .catch((e: unknown) => String(e))
        : null;
      // What the page received while the picker was open (recordInput()).
      const input = await inputSince(0).catch((e: unknown) => String(e));
      throw new Error(
        `the split didn't add a pane: ${JSON.stringify({ toasts: [...seen], pickerOpened: opened ?? null, inCore, state, input })}`,
      );
    });
  await waitForShell(added);
  return added;
}

/** Counters from the test harness (`test_counters`). */
export async function counters(
  reset = false,
): Promise<{ ptyResizes: number; ptyResizesByPane: Record<string, number> }> {
  return invoke("test_counters", { reset });
}

let rawSeq = 0;

/** Puts the pane's tty in raw mode and records the next `count` bytes it
 * receives (or what arrived within `seconds`) as hex. */
export async function rawRead(
  pane: string,
  count: number,
  seconds = 10,
  opts: { bracketed?: boolean } = {},
): Promise<() => Promise<string>> {
  const tag = `raw${++rawSeq}`;
  const out = `/tmp/tandem-e2e-raw-${process.pid}-${tag}`;
  rmSync(out, { force: true });
  rmSync(`${out}.done`, { force: true });
  // `rea''dy` keeps the typed command line from matching the marker.
  await sendToPane(
    pane,
    `stty raw -echo; ${opts.bracketed ? "printf '\\033[?2004h'; " : ""}printf 'rea''dy-${tag}\\r\\n'; timeout ${seconds} dd bs=1 count=${count} 2>/dev/null | od -An -tx1 | tr -s ' \\n' ' ' > ${out}; ${opts.bracketed ? "printf '\\033[?2004l'; " : ""}stty sane; touch ${out}.done\n`,
  );
  await waitForPaneText(pane, `ready-${tag}`);
  return async () => {
    await browser.waitUntil(() => existsSync(`${out}.done`), {
      timeout: (seconds + 5) * 1000,
      timeoutMsg: `the read of ${count} bytes never finished`,
    });
    const hex = readFileSync(out, "utf8").trim();
    rmSync(out, { force: true });
    rmSync(`${out}.done`, { force: true });
    return hex;
  };
}

// ── M4: engines ────────────────────────────────────────────────────────────

const PROFILES = [".zprofile", ".profile", ".bash_profile"];
let defaultProfile: string | null = null;

/** The test user's home (run.ts points HOME at it). */
export function userHome(): string {
  return process.env.HOME ?? "";
}

/** Rewrites the login profiles; `null` puts the run's default back (fake
 * `claude` on PATH, test logs). */
export function setProfile(lines: string[] | null): void {
  const home = userHome();
  defaultProfile ??= readFileSync(join(home, ".zprofile"), "utf8");
  const body = lines === null ? defaultProfile : `${lines.join("\n")}\n`;
  for (const f of PROFILES) writeFileSync(join(home, f), body);
}

/** The default profile without the fake-engine folder on PATH. */
export function profileWithoutEngines(): string[] {
  const home = userHome();
  defaultProfile ??= readFileSync(join(home, ".zprofile"), "utf8");
  return defaultProfile.split("\n").filter((l) => l && !l.startsWith("export PATH="));
}

/** Puts `fake-engine` in `dir` under `name` (it behaves by the name it runs as). */
export function fakeEngine(name: string, dir: string): string {
  mkdirSync(dir, { recursive: true });
  const link = join(dir, name);
  rmSync(link, { force: true });
  symlinkSync(process.env.TANDEM_FAKE_ENGINE_BIN ?? "", link);
  return link;
}

/** Engine registry overrides in the API's signed config (`/__test/config`). */
export async function setRegistryOverrides(overrides: unknown[]): Promise<void> {
  const res = await fetch(`${API}/__test/config`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ registry_overrides: overrides }),
  });
  if (!res.ok) throw new Error(`setting registry overrides failed: ${res.status} ${await res.text()}`);
}

/** Check again: re-reads the registry and the login env, then detects. */
export async function checkAgain(): Promise<{
  found: Array<{ engineId: string; source: string }>;
  slowShell: unknown;
  shellsAsked: string[];
}> {
  return invoke("engines_detect", {});
}

/** The status a pane's header shows (`working`, `waiting`, `done`, `idle`…). */
export async function paneStatus(paneId: string): Promise<string> {
  return String(
    await browser.execute(
      (id: string) =>
        document.querySelector<HTMLElement>(
          `[data-testid="pane"][data-pane-id="${id}"] [data-testid="pane-status"]`,
        )?.dataset.status ?? "",
      paneId,
    ),
  );
}

/** Waits for a pane's header status. */
export async function waitForStatus(paneId: string, status: string, timeout = 15_000): Promise<void> {
  await waitFor(
    () => paneStatus(paneId),
    (now) => now === status,
    `pane ${paneId} never showed status ${status}`,
    {
      timeout,
      interval: 100,
    },
  );
}

/** Lines of the fake engines' log (TANDEM_FAKE_ENGINE_LOG), and of tandem-hook's posts. */
export function fakeLog(): string[] {
  const path = join(process.env.TANDEM_HOME ?? "", "fake-engine.log");
  return existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : [];
}
export function hookLog(): string[] {
  const path = join(process.env.TANDEM_HOME ?? "", "hook.log");
  return existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : [];
}

/** kill -9 the app (a crash), and wait until it's gone. */
export function crash(): void {
  const app = process.env.TANDEM_APP_BINARY ?? "";
  try {
    execFileSync("pkill", ["-9", "-f", `^${app}`]);
  } catch {
    // Already gone.
  }
  for (let i = 0; i < 50; i++) {
    try {
      execFileSync("pgrep", ["-f", `^${app}`]);
    } catch {
      return;
    }
    execFileSync("sleep", ["0.1"]);
  }
}

export type SpawnRecord = { argv: string[]; envKeys: string[]; providerHome: string | null; cwd: string };

/** Spawns that started an engine in a provider home (not plain shells). */
export async function engineSpawns(): Promise<SpawnRecord[]> {
  return (await invoke<SpawnRecord[]>("test_spawn_log")).filter((r) =>
    r.envKeys.includes("CLAUDE_CONFIG_DIR"),
  );
}

/** Pane ids of the paused agent cards on screen, by card state. */
export async function pausedCards(): Promise<Array<{ paneId: string; state: string }>> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="paused-card"]')).map((el) => ({
      paneId: el.closest<HTMLElement>('[data-testid="pane"]')?.dataset.paneId ?? "",
      state: el.dataset.state ?? "",
    })),
  );
}

/** Waits until `n` new SessionStart hooks ran after `from` log lines, then
 * a moment for their session ids to be stored. */
export async function waitForSessionStarts(from: number, n: number): Promise<void> {
  await waitFor(
    () =>
      fakeLog()
        .slice(from)
        .filter((l) => l === "hook: SessionStart").length,
    (started) => started >= n,
    `agents that reported a session, of ${n}`,
    { timeout: 20_000 },
  );
  await browser.pause(800);
}

/** Stops what an earlier spec file left running (its app was closed with
 * panes open), so this one starts without the "still running" dialog. */
export async function stopLeftovers(): Promise<void> {
  await waitForApp();
  const left = await invoke<unknown[]>("orphans_list");
  if (left.length > 0) await invoke("orphans_resolve", { action: "stop" });
}

/** Closes every pane of this window (an `after` hook's cleanup). */
export async function closeAllPanes(): Promise<void> {
  for (const id of await paneIds()) await invoke("pane_close", { paneId: id });
}

/** Leftover processes from the crash are stopped without the dialog. */
export async function stopOrphansAutomatically(on: boolean): Promise<void> {
  await invoke("settings_set", { key: "orphan_policy", value: on ? "always" : "ask" });
}

/** Clicks an approval's Allow (F-TL-12.12). Allow ignores a click until its
 * place on screen has held still for half a second, and WebDriver scrolls a
 * button into view before clicking, which moves it: so scroll first, give it
 * the half second, then click once it's enabled. */
export async function clickAllow(selector: string): Promise<void> {
  const button = $(selector);
  await button.scrollIntoView({ block: "center" });
  await browser.pause(700);
  await button.waitForEnabled();
  await button.click();
}
