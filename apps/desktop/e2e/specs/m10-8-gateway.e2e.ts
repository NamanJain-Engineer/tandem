import { browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, paneIds } from "../support/app";
import { connectTestPlugin, gatewayMetrics, gatewayPort, sendAndWait } from "../support/agents";

type Thread = { id: string };

async function newThread(workspaceId: string): Promise<string> {
  const t = await invoke<Thread>("thread_create", {
    args: { kind: "thread", workspaceId, engineId: "claude" },
  });
  return t.id;
}

async function grant(threadId: string, connectionId: string): Promise<void> {
  await invoke("grants_set", { subject: { type: "thread", threadId }, connectionId, granted: true });
}

// E2E-M10-8 Gateway (F-PL-01): one listener for every session, 401 without
// a lease, a lease can't use another session's plugin, and three granted
// plugins list as the search and call tools plus the built-ins. Teammate
// built-ins are covered in E2E-M10-6.
describe("E2E-M10-8 gateway", () => {
  let ws = "";
  let echo = "";
  let upper = "";
  let down = "";

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m10-8-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [w] = await invoke<Array<{ id: string }>>("workspace_list");
    ws = w?.id ?? "";
    echo = await connectTestPlugin("test_echo");
    upper = await connectTestPlugin("test_upper");
    down = await connectTestPlugin("test_down");
  });

  it("F-PL-01.1 a request without a lease gets 401", async () => {
    const port = gatewayPort();
    expect(port).toBe((await gatewayMetrics()).metrics.port);
    const res = await fetch(`http://127.0.0.1:${port}/mcp`, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json, text/event-stream" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list" }),
    });
    expect(res.status).toBe(401);
    const forged = await fetch(`http://127.0.0.1:${port}/mcp`, {
      method: "POST",
      headers: { authorization: "Bearer not-a-lease", "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list" }),
    });
    expect(forged.status).toBe(401);
  });

  it("F-PL-01.1 a lease can't call a plugin granted only to another session", async function () {
    this.timeout(90_000);
    const a = await newThread(ws);
    const b = await newThread(ws);
    await grant(a, echo);
    await grant(b, upper);
    const refused = await sendAndWait(
      a,
      'mcp tandem__call {"tool":"test_upper__upper","args":{"text":"hi"}}',
    );
    expect(refused.join(" ")).toContain(
      "The tool failed: The test_upper plugin isn't enabled for this session",
    );
    const own = await sendAndWait(a, 'mcp tandem__call {"tool":"test_echo__echo","args":{"text":"mine"}}');
    // A vendor's answer reaches the engine labelled untrusted (F-TL-06 rule 8).
    expect(own.join(" ")).toMatch(/The tool said:[\s\S]*Untrusted content from the Test Echo plugin[\s\S]*mine/);
    const theirs = await sendAndWait(b, 'mcp tandem__call {"tool":"test_upper__upper","args":{"text":"hi"}}');
    expect(theirs.join(" ")).toMatch(/The tool said:[\s\S]*Test Upper plugin[\s\S]*HI/);
  });

  it("F-PL-01.3 with 3 plugins granted, tools/list is the search and call tools plus the built-ins", async function () {
    this.timeout(60_000);
    const t = await newThread(ws);
    for (const c of [echo, upper, down]) await grant(t, c);
    const listed = await sendAndWait(t, "tools");
    expect(listed.join(" ")).toContain(
      "Tools: tandem__call, tandem__search_tools, tandem_approvals__request",
    );
    // The vendor that's down doesn't break the others.
    const ok = await sendAndWait(t, 'mcp tandem__call {"tool":"test_upper__upper","args":{"text":"up"}}');
    expect(ok.join(" ")).toMatch(/The tool said:[\s\S]*Test Upper plugin[\s\S]*UP/);
  });

  it("F-PL-01.2 ten sessions use one gateway listener", async function () {
    this.timeout(180_000);
    const before = await gatewayMetrics();
    const threads: string[] = [];
    for (let i = 0; i < 10; i++) threads.push(await newThread(ws));
    await Promise.all(threads.map((t) => sendAndWait(t, "tools", 60_000)));
    const after = await gatewayMetrics();
    expect(after.starts).toBe(1);
    expect(after.metrics.listeners).toBe(1);
    expect(after.metrics.port).toBe(before.metrics.port);
    expect(after.metrics.leases).toBeGreaterThanOrEqual(before.metrics.leases + 10);
    expect(gatewayPort()).toBe(after.metrics.port);
  });
});
