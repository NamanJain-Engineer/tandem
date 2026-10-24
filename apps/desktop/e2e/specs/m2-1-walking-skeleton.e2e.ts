import { $, browser } from "@wdio/globals";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import {
  addFolder,
  browserLog,
  freshSignedOut,
  planFooter,
  signInDesktop,
  signUpVerified,
  terminalLines,
  terminalText,
  typeInTerminal,
  uniqueEmail,
  waitForApp,
  workspaceRoots,
} from "../support/app";

// E2E-M2-1 Walking skeleton (MILESTONES M2).
describe("E2E-M2-1 walking skeleton", () => {
  const email = uniqueEmail("skeleton");
  const ws = `/tmp/tandem-e2e-ws1-${Date.now()}`;

  it("signs up on the web and verifies through mailpit", async () => {
    await signUpVerified(email);
  });

  it("launches fresh at sign-in, which passes axe", async () => {
    await waitForApp();
    // Spec files run in name order, and `m13-*` and `m16-*` come first: they leave other profiles.
    await freshSignedOut();
    await $("#signin-title").waitForDisplayed({ timeout: 20_000 });
    expect(await $("#signin-title").getText()).toBe("Welcome to Vibezeo");
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("signs in through the scripted browser and shows Free", async () => {
    await signInDesktop(email);
    expect(browserLog()).toContain("You can return to Vibezeo.");
    await browser.waitUntil(async () => (await planFooter()) === "Free", {
      timeout: 15_000,
      timeoutMsg: `footer: ${await planFooter()}`,
    });
  });

  it("opens a workspace with a live shell that runs commands", async () => {
    await addFolder(ws);
    await browser.waitUntil(async () => (await workspaceRoots()).includes(ws), { timeout: 15_000 });
    await browser.waitUntil(async () => (await terminalText()).length > 0, {
      timeout: 20_000,
      timeoutMsg: "no shell output",
    });
    await typeInTerminal("echo hi\n");
    await browser.waitUntil(async () => (await terminalLines()).includes("hi"), {
      timeout: 15_000,
      timeoutMsg: `terminal: ${await terminalText()}`,
    });
  });

  it("the shell passes axe in both themes", async () => {
    await expectNoSeriousA11yViolationsInBothThemes();
  });
});
