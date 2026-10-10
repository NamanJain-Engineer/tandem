import { $, browser } from "@wdio/globals";
import { existsSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { home, invoke, mod } from "./app";

/** The mock authorization server and plugin upstreams (`mock-plugins`, D-954). */
export const MOCK = "http://127.0.0.1:8791";

export type MockState = {
  calls: Record<string, number>;
  access: string[];
  refresh: string[];
  revoked: string[];
  clients: number;
  paths: string[];
};

export async function mockReset(): Promise<void> {
  await fetch(`${MOCK}/__reset`);
}

/** Flips the mock's failure switches (`private_redirect=1`, `upstream_rejects=1`, …). */
export async function mockSwitch(switches: Record<string, string>): Promise<void> {
  await fetch(`${MOCK}/__switch?${new URLSearchParams(switches).toString()}`);
}

export async function mockState(): Promise<MockState> {
  return (await (await fetch(`${MOCK}/__state`)).json()) as MockState;
}

export async function mockCalls(tool: string): Promise<number> {
  return (await mockState()).calls[tool] ?? 0;
}

/** Revokes a token at the mock, as the provider would. */
export async function mockRevoke(token: string): Promise<void> {
  await fetch(`${MOCK}/revoke`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ token }).toString(),
  });
}

/** "hold": the scripted browser writes the sign-in URL down instead of
 * finishing it (`plugin-authorize.txt`), so a spec can call back late. */
export function browserSignIn(mode: "follow" | "hold"): void {
  const file = join(home(), "plugin-signin.txt");
  rmSync(join(home(), "plugin-authorize.txt"), { force: true });
  if (mode === "follow") rmSync(file, { force: true });
  else writeFileSync(file, "hold");
}

export async function heldSignInUrl(): Promise<string> {
  const file = join(home(), "plugin-authorize.txt");
  await browser.waitUntil(async () => existsSync(file), {
    timeout: 20_000,
    timeoutMsg: "the browser was never opened",
  });
  return readFileSync(file, "utf8").trim();
}

export type Card = {
  pluginId: string;
  label: string;
  state: string;
  status: string;
  connectionId: string | null;
  accountLabel: string | null;
};

export async function catalog(): Promise<Card[]> {
  return invoke<Card[]>("plugins_catalog");
}

export async function connectionOf(pluginId: string): Promise<Card | undefined> {
  return (await catalog()).find((c) => c.pluginId === pluginId && c.connectionId);
}

/** Settings › Plugins. */
export async function openPlugins(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Plugins"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="plugins-page"]').waitForDisplayed({ timeout: 10_000 });
}

/** Radix tabs switch on mousedown, not click. */
export async function openTabNamed(scope: string, name: string): Promise<void> {
  const clicked = await browser.execute(
    (s: string, n: string) => {
      const tab = Array.from(document.querySelectorAll<HTMLElement>(`${s} [role="tab"]`)).find((el) =>
        (el.textContent ?? "").trim().startsWith(n),
      );
      tab?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      tab?.click();
      return !!tab;
    },
    scope,
    name,
  );
  if (!clicked) throw new Error(`no tab named ${name} in ${scope}`);
}

/** Connects GitHub through the mock with the browser sign-in (over IPC;
 * E2E-M12-1 drives the sheet). Its connection id. */
export async function connectGithub(): Promise<string> {
  browserSignIn("follow");
  await invoke("plugin_connect_oauth", { pluginId: "github", connectionId: null });
  let id = "";
  await browser.waitUntil(
    async () => {
      const c = await connectionOf("github");
      id = c?.state === "connected" ? (c.connectionId ?? "") : "";
      return !!id;
    },
    { timeout: 30_000, timeoutMsg: "GitHub never connected" },
  );
  return id;
}

/** Every file under `dir` that contains `needle` (credentials must live
 * only in the keychain, which E2E keeps in memory). */
export function filesContaining(dir: string, needle: string): string[] {
  const out: string[] = [];
  const walk = (d: string) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name);
      let st;
      try {
        st = statSync(p);
      } catch {
        continue;
      }
      if (st.isDirectory()) walk(p);
      else if (st.isFile() && st.size < 64 * 1024 * 1024) {
        try {
          if (readFileSync(p).includes(needle)) out.push(p);
        } catch {
          // Sockets and files that vanished.
        }
      }
    }
  };
  walk(dir);
  return out;
}
