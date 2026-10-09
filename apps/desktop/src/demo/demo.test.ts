import { afterEach, describe, expect, it } from "vitest";
import type { Thread, ThreadChanges, ThreadDetail, ThreadEvent, ThreadEventEnvelope } from "@tandem/protocol";
import { setInvokeBackend } from "../ipc/invoke";
import { DemoCore, FIRST_MESSAGE } from "./backend";
import { DemoShell, PROMPT, run } from "./shell";

const tick = () => new Promise((r) => setTimeout(r, 5));

async function until(check: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !check(); i++) await tick();
  expect(check()).toBe(true);
}

let core: DemoCore | null = null;
afterEach(() => {
  core?.dispose();
  setInvokeBackend(null);
});

describe("browser demo (vibezeo.com/demo)", () => {
  it("F-WB-01 the demo shell answers its commands and echoes typing", () => {
    expect(run("ls")?.join(" ")).toContain("package.json");
    expect(run("git status")?.join("\n")).toContain("src/checkout/redirect.ts");
    expect(run("npm test")?.join("\n")).toContain("19 passed");
    expect(run("rm -rf /")?.[0]).toContain("command not found");
    expect(run("clear")).toBeNull();
    const shell = new DemoShell();
    expect(shell.greeting()).toContain(PROMPT);
    expect(shell.write("pw")).toBe("pw");
    expect(shell.write("x\x7f")).toBe("x\b \b");
    expect(shell.write("d\r")).toBe(`d\r\n/home/you/shop\r\n${PROMPT}`);
  });

  it("F-WB-01 the first thread sends its draft, asks before the tests, and reports the fix with its changes", async () => {
    core = new DemoCore(0);
    const threads = core.handle("thread_list", {}) as Thread[];
    const first = threads.find((t) => t.id === core?.firstThread);
    expect(first?.title).toBe("Fix the redirect loop after login");
    const detail = core.handle("thread_get", { threadId: core.firstThread }) as ThreadDetail;
    expect(JSON.parse(detail.draft)).toEqual({ text: FIRST_MESSAGE, attachmentIds: [] });

    const seen: ThreadEvent[] = [];
    core.handle("thread_subscribe", {
      threadId: core.firstThread,
      channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) },
    });
    core.handle("thread_send", { threadId: core.firstThread, text: FIRST_MESSAGE, attachments: [] });
    await until(() => seen.some((e) => e.type === "approval_request"));
    const ask = seen.find((e) => e.type === "approval_request");
    expect(ask?.type === "approval_request" && ask.detail).toBe("npm test -- src/checkout");
    // Nothing more happens until the visitor answers.
    await tick();
    expect(seen.at(-1)?.type).toBe("approval_request");

    core.handle("approval_resolve", {
      approvalId: ask?.type === "approval_request" ? ask.id : "",
      decision: { type: "allow", scope: "once" },
    });
    await until(() => seen.some((e) => e.type === "turn_end"));
    const reply = seen.find((e) => e.type === "assistant");
    expect(reply?.type === "assistant" && reply.markdown).toContain("Fixed the redirect loop.");
    const changes = core.handle("thread_changes", { threadId: core.firstThread }) as ThreadChanges;
    expect(changes.files.map((f) => f.path)).toEqual([
      "src/checkout/redirect.test.ts",
      "src/checkout/redirect.ts",
    ]);

    // Later messages get a short simulated answer.
    core.handle("thread_send", { threadId: core.firstThread, text: "make a plan", attachments: [] });
    await until(() => seen.filter((e) => e.type === "turn_end").length === 2);
    expect(JSON.stringify(seen.filter((e) => e.type === "assistant").at(-1))).toContain("Here's a plan");
  });

  it("F-WB-01 an engine switch in the demo shows the divider above the next message", async () => {
    core = new DemoCore(0);
    const id = core.firstThread;
    const seen: ThreadEvent[] = [];
    core.handle("thread_subscribe", { threadId: id, channel: { onmessage: (e: ThreadEventEnvelope) => seen.push(e.event) } });
    core.handle("thread_send", { threadId: id, text: "hi", attachments: [] });
    core.handle("thread_stop", { threadId: id });
    core.handle("thread_switch_engine", { threadId: id, engineId: "codex" });
    core.handle("thread_send", { threadId: id, text: "again", attachments: [] });
    const at = seen.findIndex((e) => e.type === "user" && e.text === "again");
    expect(seen[at + 1]).toEqual({ type: "divider", text: "Continued on Codex (Default) with a summary" });
    core.handle("thread_stop", { threadId: id });
  });

  it("F-WB-01 commands the demo doesn't simulate are quiet, and lists stay lists", () => {
    core = new DemoCore(0);
    expect(core.handle("presets_list", {})).toEqual([]);
    expect(core.handle("workspace_remove", {})).toBeNull();
    expect(() => core?.handle("thread_get", { threadId: "nope" })).toThrow();
  });
});
