import { browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { call, freshUser, invoke, planFooter, signUpVerified, uniqueEmail } from "../support/app";
import { testRoute, titlebarCredits, waitForEntitlement } from "../support/hosted";

const FIXTURE = fileURLToPath(new URL("../../../../scripts/stripe-fixture.sh", import.meta.url));

type Page = { items: Array<{ kind: string; state: string; title: string }> };
const creditItem = async () =>
  (await invoke<Page>("notifications_list", {})).items.find((n) => n.kind === "credit_low" && n.state !== "resolved");

// E2E-M13-3 packs on the desktop, F-SH-01 the ◈ display, F-DB-02 the
// credit-low item, and E2E-M13-7 a refund (MILESTONES M13).
describe("E2E-M13-3 credits on the desktop", () => {
  let email = "";

  before(async function () {
    this.timeout(120_000);
    ({ email } = await freshUser("credits"));
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await waitForEntitlement((e) => e.plan === "pro" && e.credits?.monthly === 10_000, "Pro with 10,000 credits");
  });

  it("F-SH-01 the title bar shows ◈ and the balance", async () => {
    await browser.waitUntil(async () => (await titlebarCredits()).text === "◈10,000", {
      timeout: 5_000,
      timeoutMsg: JSON.stringify(await titlebarCredits()),
    });
    expect((await titlebarCredits()).level).toBe("normal");
  });

  it("E2E-M13-3 a 5,000 pack, its webhook replayed, adds 5,000 once and shows within 5 s", async function () {
    this.timeout(60_000);
    const nonce = String(Date.now());
    const started = Date.now();
    for (let i = 0; i < 2; i++) {
      execFileSync("bash", [FIXTURE, "checkout.session.completed", email, nonce], { stdio: "ignore" });
    }
    await browser.waitUntil(async () => (await titlebarCredits()).text === "◈15,000", {
      timeout: 5_000 + (Date.now() - started),
      timeoutMsg: `title bar: ${JSON.stringify(await titlebarCredits())}`,
    });
    // Still once, a moment later.
    await browser.pause(1_000);
    expect((await titlebarCredits()).text).toBe("◈15,000");
  });

  it("F-DB-02 at 10% of the grant the display warns and the bell lists it; at 0 it's danger", async function () {
    this.timeout(60_000);
    await testRoute("credits", { email, monthly: 900, purchased: 0 });
    await browser.waitUntil(async () => (await titlebarCredits()).level === "warning", {
      timeout: 5_000,
      timeoutMsg: JSON.stringify(await titlebarCredits()),
    });
    await browser.waitUntil(async () => (await creditItem()) !== undefined, { timeout: 5_000, timeoutMsg: "no credit-low item" });
    expect((await creditItem())?.title).toBe("900 credits left");
    // Another refresh while still low doesn't add a second item.
    await testRoute("credits", { email, monthly: 800, purchased: 0 });
    await browser.waitUntil(async () => (await titlebarCredits()).text === "◈800", { timeout: 5_000 });
    const open = (await invoke<Page>("notifications_list", {})).items.filter(
      (n) => n.kind === "credit_low" && n.state !== "resolved",
    );
    expect(open).toHaveLength(1);

    await testRoute("credits", { email, monthly: 0, purchased: 0 });
    await browser.waitUntil(async () => (await titlebarCredits()).level === "danger", { timeout: 5_000 });
    expect((await titlebarCredits()).text).toBe("◈0");

    // Back above 10%: the item resolves.
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await browser.waitUntil(async () => (await creditItem()) === undefined, { timeout: 5_000, timeoutMsg: "still low" });
    expect((await titlebarCredits()).level).toBe("normal");
  });

  it("E2E-M13-7 an admin refund: Free limits and a monthly bucket of 0", async function () {
    this.timeout(60_000);
    const adminEmail = uniqueEmail("admin");
    const admin = await signUpVerified(adminEmail);
    await testRoute("admin", { email: adminEmail });
    const { id } = await testRoute<{ id: string }>(`user?email=${encodeURIComponent(email)}`);
    const res = await call(admin, "POST", `/v1/admin/users/${id}/refund`, { reason: "E2E: asked within 14 days" });
    expect(res.status).toBe(200);
    await waitForEntitlement((e) => e.plan === "free" && (e.credits?.monthly ?? -1) === 0, "Free after the refund", 5_000);
    expect(await planFooter()).toBe("FREE");
    const limits = (await invoke<{ limits: { panes: number | null } }>("entitlement_get")).limits;
    expect(limits.panes).toBe(4);
  });
});
