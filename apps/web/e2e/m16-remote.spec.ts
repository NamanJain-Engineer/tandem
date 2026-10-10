import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { API, signIn } from "./helpers";
import { FakeDesktop } from "./remoteDesktop";

/** A desktop bearer for pro@tandem.test, through the device-code flow (F-BE-03). */
async function desktopBearer(page: Page, request: APIRequestContext): Promise<string> {
  const start = await request.post(`${API}/v1/auth/device/start`, {
    data: { client: "Vibezeo E2E", version: "0.4.0" },
    headers: { "x-forwarded-for": `198.51.100.${Math.floor(Math.random() * 200) + 1}` },
  });
  const s = (await start.json()) as { device_code: string; verification_uri_complete: string };
  await signIn(page, "pro@tandem.test", undefined, "/account");
  const url = new URL(s.verification_uri_complete);
  await page.goto(url.pathname + url.search);
  await page.getByRole("button", { name: "Approve" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Approved" })).toBeVisible();
  await page.waitForTimeout(5_500);
  const token = await request.post(`${API}/v1/auth/device/token`, { data: { device_code: s.device_code } });
  return ((await token.json()) as { access_token: string }).access_token;
}

test.describe("@m16 phone companion PWA", () => {
  test("E2E F-RM-03 with nothing paired, /remote explains how to pair @m16", async ({ page }) => {
    await page.goto("/remote");
    await expect(page.getByRole("heading", { name: "Pair this phone" })).toBeVisible();
    await expect(page.getByText("end-to-end encrypted")).toBeVisible();
  });

  test("E2E F-RM-03 pairs from the QR link, shows agents, queues an allow, sends to a thread, and wipes itself on revoke @m16", async ({
    page,
    request,
    context,
  }) => {
    test.setTimeout(90_000);
    const bearer = await desktopBearer(page, request);
    const desktop = new FakeDesktop(bearer);
    const link = await desktop.start("http://localhost:3000");

    // A fresh browser profile is the phone.
    const phone = await context.browser()!.newContext();
    const p = await phone.newPage();
    const u = new URL(link);
    await p.goto(u.pathname + u.hash);
    await expect(p.getByTestId("remote-connection")).toHaveText("Work laptop");
    // The pairing secret leaves the address bar.
    expect(new URL(p.url()).hash).toBe("");
    expect(desktop.received[0]).toMatchObject({ t: "hi" });

    await expect(p.getByTestId("remote-agents")).toContainText("auth tests");
    await expect(p.getByTestId("remote-agents")).toContainText("Running the login tests");

    await p.getByRole("radio", { name: /Approvals/ }).click();
    await expect(p.getByTestId("remote-approvals")).toContainText("Run `pnpm deploy`");
    await p.getByRole("button", { name: "Allow" }).click();
    await expect(p.getByText("Sent. It applies once you confirm it on your computer.")).toBeVisible();
    await expect(p.getByText("Waiting for you on your computer")).toBeVisible();
    expect(desktop.received.some((m) => m.cmd === "approval.decide" && m.args?.decision === "allow")).toBe(true);

    await p.getByRole("radio", { name: "Threads" }).click();
    await p.getByRole("button", { name: /Fix the login bug/ }).click();
    await expect(p.getByTestId("remote-transcript")).toContainText("The session cookie is set on the wrong path.");
    await p.getByRole("textbox", { name: "Message" }).fill("Try the root path");
    await p.getByRole("button", { name: "Send" }).click();
    await expect.poll(() => desktop.received.some((m) => m.cmd === "thread.send" && m.args?.text === "Try the root path")).toBe(true);

    // A reload resumes the pairing from IndexedDB.
    await p.reload();
    await expect(p.getByTestId("remote-connection")).toHaveText("Work laptop");
    const stored = await p.evaluate(
      () =>
        new Promise<string>((resolve) => {
          const req = indexedDB.open("vibezeo-remote");
          req.onsuccess = () => {
            const tx = req.result.transaction("kv").objectStore("kv").get("pairing");
            tx.onsuccess = () => resolve(JSON.stringify(Object.keys(tx.result ?? {})));
          };
        }),
    );
    // Only the sealed form is stored: an IV and ciphertext.
    expect(stored).toBe('["iv","ct"]');

    // Revoked on the desktop: the phone hears bye and forgets the pairing.
    desktop.send({ t: "bye", reason: "revoked" });
    await expect(p.getByText("This phone is no longer paired.")).toBeVisible();
    await p.reload();
    await expect(p.getByRole("heading", { name: "Pair this phone" })).toBeVisible();
    desktop.close();
    await phone.close();
  });
});
