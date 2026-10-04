import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Jar, json, makeTestApp, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

describe("F-BE-01 web accounts and auth", () => {
  it("F-BE-01.1 sign up, verify by email, sign in, see /me", async () => {
    const { jar } = await signUpVerified(t, "new@tandem.test");
    const me = await json(t, "GET", "/v1/me", { jar });
    expect(me.status).toBe(200);
    expect(await me.json()).toMatchObject({
      user: { email: "new@tandem.test", emailVerified: true },
      plan: "free",
    });
  });

  it("sign-in is refused until the email is verified", async () => {
    const jar = new Jar();
    await json(t, "POST", "/v1/auth/sign-up/email", {
      body: { email: "unverified@tandem.test", password: "Correct-horse-42", name: "U" },
      jar,
    });
    const signin = await json(t, "POST", "/v1/auth/sign-in/email", {
      body: { email: "unverified@tandem.test", password: "Correct-horse-42" },
    });
    expect(signin.status).toBe(403);
  });

  it("F-BE-01.2 a cookie-authenticated POST without the CSRF token is rejected with 403", async () => {
    const { jar } = await signUpVerified(t, "csrf@tandem.test");
    const body = { currentPassword: "Correct-horse-42", newPassword: "Another-horse-43" };
    const missing = await json(t, "POST", "/v1/auth/change-password", { body, jar, csrf: false });
    expect(missing.status).toBe(403);
    expect(await missing.json()).toMatchObject({ code: "FORBIDDEN_CSRF" });
    const ok = await json(t, "POST", "/v1/auth/change-password", { body, jar });
    expect(ok.status).toBe(200);
  });

  it("F-BE-01.3 the password 'password123' is rejected as breached", async () => {
    const res = await json(t, "POST", "/v1/auth/sign-up/email", {
      body: { email: "breach@tandem.test", password: "password123", name: "B" },
    });
    expect(res.status).toBe(400);
    expect(JSON.stringify(await res.json())).toMatch(/data breach/);
  });

  it("passwords shorter than 10 characters are rejected", async () => {
    const res = await json(t, "POST", "/v1/auth/sign-up/email", {
      body: { email: "short@tandem.test", password: "Short-1", name: "S" },
    });
    expect(res.status).toBe(400);
  });

  it("password reset links expire after 30 minutes", async () => {
    await signUpVerified(t, "reset@tandem.test");
    const res = await json(t, "POST", "/v1/auth/request-password-reset", {
      body: { email: "reset@tandem.test", redirectTo: "/reset" },
    });
    expect(res.status).toBe(200);
    const mail = t.email.latest("reset@tandem.test");
    expect(mail?.subject).toBe("Reset your Tandem password");
    expect(mail?.text).toMatch(/30 minutes/);
  });

  it("DELETE /v1/me records a deletion request", async () => {
    const { jar } = await signUpVerified(t, "delete@tandem.test");
    const res = await json(t, "DELETE", "/v1/me", { body: { confirm: "DELETE" }, jar });
    expect(res.status).toBe(202);
    expect(await res.json()).toMatchObject({ completes_within_days: 30 });
  });
});
