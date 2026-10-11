import { $, browser } from "@wdio/globals";
import {
  addFolder,
  freshOffline,
  freshUser,
  invoke,
  mod,
  paneIds,
  planFooter,
  sendToPane,
  setPlan,
  split,
  typeKeys,
  waitForPaneText,
} from "../support/app";
import { entitlement, testRoute } from "../support/hosted";

const pane = (id: string) => `[data-testid="pane"][data-pane-id="${id}"]`;
const attr = async (id: string, name: string) => $(pane(id)).getAttribute(name);

/** The palette's options, as their text. */
const options = async () =>
  (await browser.execute(() =>
    Array.from(document.querySelectorAll('[role="dialog"] [role="option"]')).map((o) =>
      (o.textContent ?? "").trim(),
    ),
  )) as string[];

type Found = { matches: Array<{ label: string; score: number }>; clear: boolean; ranking: "local" | "hosted" };

/** Three panes, "auth tests", "billing UI" and "docs", in a new folder. */
async function threePanes(tag: string): Promise<{ auth: string; docs: string }> {
  await addFolder(`/tmp/tandem-e2e-m16-2-${tag}-${Date.now()}`);
  await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  const [auth = ""] = await paneIds();
  const billing = await split("right");
  const docs = await split("down");
  for (const [id, name] of [
    [auth, "auth tests"],
    [billing, "billing UI"],
    [docs, "docs"],
  ] as const) {
    await invoke("pane_rename", { paneId: id, humanName: name });
  }
  await sendToPane(auth, "echo 12 passing\r");
  await waitForPaneText(auth, "12 passing");
  return { auth, docs };
}

// E2E-M16-2 Find work (F-CN-05). The search runs on this computer, so it
// finds the same with the server down: the first case has no server at all;
// the second asks the hosted ranking (the API's mock model), then takes the
// server down.
describe("E2E-M16-2 find work", () => {
  it('F-CN-05.1 "login testing" finds the pane named auth tests, then shows and flashes it', async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m16-2-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [auth = ""] = await paneIds();
    const billing = await split("right");
    const docs = await split("down");
    for (const [id, name] of [
      [auth, "auth tests"],
      [billing, "billing UI"],
      [docs, "docs"],
    ] as const) {
      await invoke("pane_rename", { paneId: id, humanName: name });
    }
    await sendToPane(auth, "echo 12 passing\r");
    await waitForPaneText(auth, "12 passing");
    // One clear match. (The folder, in every prompt, says "e2e", which means
    // "test", so the other two panes match weakly further down.)
    const result = await invoke<{ matches: Array<{ label: string; score: number }>; clear: boolean }>(
      "find_work",
      {
        query: "login testing",
      },
    );
    expect(result.clear).toBe(true);
    expect(result.matches[0]?.label).toBe("auth tests");
    expect(result.matches.slice(1).every((m) => m.score * 1.5 < (result.matches[0]?.score ?? 0))).toBe(true);
    await $(pane(docs)).click();
    await browser.waitUntil(async () => (await attr(docs, "data-focused")) === "true", { timeout: 5_000 });

    // "Find work…" from the command palette turns it into the search.
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), "k"]);
    await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 3_000 });
    await typeKeys("Find work");
    await browser.keys("Enter");
    const search = $('input[placeholder="Describe the work, like “the login tests”"]');
    await search.waitForDisplayed({ timeout: 3_000 });
    await typeKeys("login testing");
    await browser.waitUntil(async () => (await options()).some((o) => o.startsWith("auth tests")), {
      timeout: 5_000,
      timeoutMsg: `no "auth tests" among ${JSON.stringify(await options())}`,
    });
    expect((await options())[0]).toMatch(/^auth tests/);

    await browser.keys("Enter");
    await browser.waitUntil(async () => (await attr(auth, "data-focused")) === "true", {
      timeout: 5_000,
      timeoutMsg: "the auth tests pane didn't get focus",
    });
    expect(await attr(auth, "data-flash")).toBe("true");
    expect(await attr(docs, "data-focused")).toBeNull();

    // The next Mod+K opens the commands again.
    await browser.keys([mod(), "k"]);
    await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 3_000 });
    await browser.keys("Escape");
  });

  it('F-CN-05.1 with the hosted ranking on, "login testing" resolves to auth tests, and the local fallback answers the same with the server down', async function () {
    this.timeout(120_000);
    const { email } = await freshUser("m16-2");
    await setPlan(email, "pro");
    await testRoute("credits", { email, monthly: 1_000, purchased: 0 });
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), {
      timeout: 20_000,
      timeoutMsg: "the plan never showed Pro",
    });
    await browser.waitUntil(async () => ((await entitlement()).credits?.monthly ?? 0) === 1_000, {
      timeout: 20_000,
      timeoutMsg: "the credits never arrived",
    });
    await threePanes("hosted");
    // Off by default: nothing is asked.
    expect((await invoke<Found>("find_work", { query: "login testing" })).ranking).toBe("local");
    await invoke("settings_set", { key: "find_work_hosted", value: true });
    const credits = async () => {
      const c = (await entitlement()).credits;
      return c ? c.monthly + c.purchased : -1;
    };
    const before = await credits();
    const hosted = await invoke<Found>("find_work", { query: "login testing" });
    expect(hosted.ranking).toBe("hosted");
    expect(hosted.clear).toBe(true);
    expect(hosted.matches[0]?.label).toBe("auth tests");
    // It cost one credit.
    await browser.waitUntil(async () => (await credits()) === before - 1, {
      timeout: 10_000,
      timeoutMsg: `credits went from ${before} to ${await credits()}`,
    });

    // The server is down: the local ranking gives the same answer.
    await invoke("test_find_work_server", { down: true });
    try {
      const local = await invoke<Found>("find_work", { query: "login testing" });
      expect(local.ranking).toBe("local");
      expect(local.clear).toBe(true);
      expect(local.matches[0]?.label).toBe("auth tests");
    } finally {
      await invoke("test_find_work_server", { down: false });
      await invoke("settings_set", { key: "find_work_hosted", value: false });
    }
  });
});
