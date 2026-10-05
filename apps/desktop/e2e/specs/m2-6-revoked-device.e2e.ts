import { $ } from "@wdio/globals";
import { deviceSessions, freshUser, invoke, revokeDevice } from "../support/app";

// E2E-M2-6 Revoked device (MILESTONES M2, F-WB-05 AC1).
describe("E2E-M2-6 revoked device", () => {
  it("revoking the device on the web brings back the sign-in prompt", async () => {
    const { jar } = await freshUser("revoke");
    const sessions = await deviceSessions(jar);
    expect(sessions.length).toBeGreaterThan(0);
    for (const s of sessions) await revokeDevice(jar, s.id);
    await invoke("test_auth_refresh");
    await $("#signin-title").waitForDisplayed({ timeout: 10_000 });
  });
});
