import { $, browser } from "@wdio/globals";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { freshUser, home, invoke, mod, setPlan } from "../support/app";
import { waitForEntitlement } from "../support/hosted";

type Summary = { teammate: { name: string; brief: string; engineId: string } };

// E2E-M10-7 Create with AI (F-AG-04, D-8): the desktop API fixture fills
// the New teammate sheet for review; nothing is saved until Create.
describe("E2E-M10-7 create with AI", () => {
  const fixture = join(home(), "teammate-draft.json");

  before(async function () {
    this.timeout(90_000);
    writeFileSync(
      fixture,
      JSON.stringify({
        name: "Dependency Keeper",
        brief: "You keep our dependencies current. Open one upgrade at a time and say what changed.",
        engine: "claude",
        plugins: ["github"],
      }),
    );
    const { email } = await freshUser("m10-7");
    await setPlan(email, "pro");
    await waitForEntitlement((e) => e.plan === "pro", "an account that carries Create with AI", 20_000);
  });

  after(() => {
    rmSync(fixture, { force: true });
  });

  it("F-AG-04.2 the draft prefills the form and nothing is saved until Create", async function () {
    this.timeout(60_000);
    const names = async () => (await invoke<Summary[]>("teammate_list")).map((s) => s.teammate.name);
    const before = await names();
    await browser.keys([mod(), "1"]);
    await $('[data-testid="teammate-new"]').click();
    const card = await $('[data-testid="draft-card"]');
    await card.waitForDisplayed({ timeout: 10_000 });
    await $('[data-testid="draft-description"]').setValue("Someone to keep our npm packages up to date");
    await $('[data-testid="draft-run"]').click();
    const name = await $('[data-testid="teammate-name-input"]');
    await browser.waitUntil(async () => (await name.getValue()) === "Dependency Keeper", {
      timeout: 15_000,
      timeoutMsg: "the draft didn't fill the name",
    });
    expect(await $('[data-testid="teammate-brief"]').getValue()).toContain(
      "You keep our dependencies current.",
    );
    expect(await $('[data-testid="engine-tile"][data-engine="claude"]').getAttribute("aria-checked")).toBe(
      "true",
    );
    // Reviewing isn't saving.
    expect(await names()).toEqual(before);
    await $('[data-testid="teammate-create"]').click();
    await browser.waitUntil(async () => (await names()).includes("Dependency Keeper"), {
      timeout: 10_000,
      timeoutMsg: "Create didn't save the teammate",
    });
    const made = (await invoke<Summary[]>("teammate_list")).find(
      (s) => s.teammate.name === "Dependency Keeper",
    );
    expect(made?.teammate.engineId).toBe("claude");
  });
});
