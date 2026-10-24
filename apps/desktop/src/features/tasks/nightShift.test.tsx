import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NightAccount } from "@tandem/protocol";
import { setInvokeBackend } from "../../ipc/invoke";
import { useNightShift } from "../../stores/nightShift";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { accountOptions, planText } from "./NightShift";

const AT = new Date(2026, 9, 16, 3, 10).getTime();
const acct = (accountId: string, engineId: string, resetsAt: number | null): NightAccount => ({
  accountId,
  engineId,
  resetsAt,
  window: resetsAt === null ? null : "5h",
});

beforeEach(() => resetStores());
afterEach(() => {
  vi.useRealTimers();
  setInvokeBackend(null);
});

describe("Night Shift (F-RT-06, UI-171)", () => {
  it("F-RT-06.1 the plan says when, on which account, and that it never uses another", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 15, 23, 0));
    const text = planText({
      task: "Write tests",
      account: "Work",
      engine: "Codex",
      at: AT,
      window: "5h",
      isolation: "lane",
      access: "edits",
    });
    expect(text).toMatch(
      /^Starts Write tests on Work \(Codex\) at Fri 3:10\s?AM, when its 5-hour window resets\. In a new worktree, with Edits access\. It waits for that account's reset and never uses another account\.$/,
    );
  });

  it("F-RT-06.5 accounts without a known reset can't be picked; engines that can't run tasks aren't listed", () => {
    const options = accountOptions(
      [acct("a1", "codex", AT), acct("a2", "claude", null), acct("a3", "aider", AT)],
      (id) => ({ a1: "Work", a2: "Home" })[id] ?? id,
      new Set(["codex", "claude"]),
    );
    expect(options.map((o) => [o.value, o.disabled])).toEqual([
      ["a1", false],
      ["a2", true],
    ]);
    expect(options[1]?.label).toBe("Home · No reset time known yet");
  });

  it("F-RT-06.4 cancel reloads the runs; errors come back", async () => {
    const calls = fakeBackend({
      night_cancel: () => null,
      night_list: () => [],
    });
    expect(await useNightShift.getState().cancel("nr_1")).toBeNull();
    expect(calls.map((c) => c.cmd)).toEqual(["night_cancel", "night_list"]);
    fakeBackend({ night_cancel: () => Promise.reject({ code: "not_found", message: "gone" }) });
    expect(await useNightShift.getState().cancel("nr_1")).toMatchObject({ message: "gone" });
  });
});
