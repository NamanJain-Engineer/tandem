import { browser } from "@wdio/globals";
import { API, invoke } from "./app";

/** Calls an API test route (`/__test/*`, M13 helpers). */
export async function testRoute<T = unknown>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${API}/__test/${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`/__test/${path}: ${res.status} ${await res.text()}`);
  return (res.status === 204 ? undefined : await res.json()) as T;
}

type Entitlement = {
  plan: string;
  policy: { maxAccess: string; auditEnabled: boolean; forceTelemetryOff: boolean } | null;
  org: { name: string; role: string } | null;
  credits: { monthly: number; purchased: number; cap: number | null } | null;
};

export async function entitlement(): Promise<Entitlement> {
  return invoke<Entitlement>("entitlement_get");
}

/** Waits until the desktop's plan (from the SSE stream) matches. */
export async function waitForEntitlement(ok: (e: Entitlement) => boolean, what: string, timeout = 10_000): Promise<void> {
  await browser.waitUntil(async () => ok(await entitlement()), {
    timeout,
    timeoutMsg: `${what}: ${JSON.stringify(await entitlement())}`,
  });
}

/** The ◈ display in the title bar: its number and level. */
export async function titlebarCredits(): Promise<{ text: string; level: string }> {
  return browser.execute(() => {
    const el = document.querySelector<HTMLElement>('[data-testid="titlebar-credits"]');
    // The glyph and the number are separate nodes: compare them as one word.
    return { text: (el?.innerText ?? "").replace(/\s+/g, ""), level: el?.dataset.level ?? "" };
  });
}

export async function auditEvents(email: string): Promise<Array<{ event: string; actor: string | null; meta: Record<string, unknown> }>> {
  return (await testRoute<{ events: Array<{ event: string; actor: string | null; meta: Record<string, unknown> }> }>(
    `audit?email=${encodeURIComponent(email)}`,
  )).events;
}
