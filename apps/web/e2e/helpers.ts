import { expect, type APIRequestContext, type Page } from "@playwright/test";

export const API = "http://127.0.0.1:8787";
export const MAILPIT = "http://localhost:8025";
export const SEED_PASSWORD = "Tandem-dev-pass-1";

export async function signIn(
  page: Page,
  email: string,
  password = SEED_PASSWORD,
  next = "/account",
): Promise<void> {
  await page.goto(`/signin?next=${encodeURIComponent(next)}`);
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.waitForURL(`**${next}`);
}

export async function testRoute(request: APIRequestContext, path: string, data: unknown): Promise<void> {
  const res = await request.post(`${API}/__test/${path}`, { data });
  expect(res.ok(), `${path}: ${res.status()}`).toBeTruthy();
}

/** The latest message to `to` in mailpit, waiting for it to arrive. */
export async function latestMail(
  request: APIRequestContext,
  to: string,
): Promise<{ Subject: string; Text: string }> {
  let found: { Subject: string; Text: string } | undefined;
  await expect
    .poll(
      async () => {
        const list = await (
          await request.get(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:${to}`)}&limit=1`)
        ).json();
        const id = list.messages?.[0]?.ID;
        if (!id) return false;
        found = await (await request.get(`${MAILPIT}/api/v1/message/${id}`)).json();
        return true;
      },
      { timeout: 15_000 },
    )
    .toBeTruthy();
  if (!found) throw new Error("no mail");
  return found;
}

export async function clearMail(request: APIRequestContext, to: string): Promise<void> {
  await request.delete(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:${to}`)}`);
}

export async function me(
  page: Page,
): Promise<{
  plan: string;
  status: string;
  limits: { panes: number | null };
  credits: { monthly: number; purchased: number };
}> {
  return page.evaluate(async () => (await fetch("/v1/me", { credentials: "include" })).json());
}
