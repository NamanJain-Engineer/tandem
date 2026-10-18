import type { FileDiff, ThreadEvent } from "@tandem/protocol";

/** One step of a simulated turn: an event after a pause, or a stop that
 * waits for the visitor to answer an approval. */
export type Step = { wait: number; event: ThreadEvent } | { approval: ThreadEvent & { type: "approval_request" } };

const tool = (
  id: string,
  verb: Extract<ThreadEvent, { type: "tool" }>["verb"],
  argSummary: string,
  status: Extract<ThreadEvent, { type: "tool" }>["status"],
  extra: Partial<Extract<ThreadEvent, { type: "tool" }>> = {},
): ThreadEvent => ({
  type: "tool",
  id,
  verb,
  argSummary,
  status,
  output: null,
  outputTruncated: false,
  outputFile: null,
  diff: null,
  detail: null,
  ...extra,
});

/** The edit the simulated agent makes, as a unified diff. */
export const CHECKOUT_PATCH = [
  "diff --git a/src/checkout/redirect.ts b/src/checkout/redirect.ts",
  "--- a/src/checkout/redirect.ts",
  "+++ b/src/checkout/redirect.ts",
  "@@ -12,7 +12,9 @@ export function nextStep(session: Session): string {",
  "   if (!session.user) {",
  "-    return `/login?next=${encodeURIComponent(session.path)}`;",
  "+    // Don't send the login page back to itself.",
  "+    const next = session.path.startsWith(\"/login\") ? \"/checkout\" : session.path;",
  "+    return `/login?next=${encodeURIComponent(next)}`;",
  "   }",
  "   return session.cart.empty ? \"/cart\" : \"/checkout/pay\";",
  " }",
  "",
].join("\n");

export const TEST_PATCH = [
  "diff --git a/src/checkout/redirect.test.ts b/src/checkout/redirect.test.ts",
  "--- a/src/checkout/redirect.test.ts",
  "+++ b/src/checkout/redirect.test.ts",
  "@@ -20,3 +20,9 @@ describe(\"nextStep\", () => {",
  "     expect(nextStep(guest(\"/cart\"))).toBe(\"/login?next=%2Fcart\");",
  "   });",
  "+",
  "+  it(\"never redirects the login page to itself\", () => {",
  "+    expect(nextStep(guest(\"/login?next=%2Flogin\"))).toBe(\"/login?next=%2Fcheckout\");",
  "+  });",
  " });",
  "",
].join("\n");

/** What the Changes panel lists once the simulated fix is in. */
export const DEMO_CHANGES: FileDiff[] = [
  {
    path: "src/checkout/redirect.test.ts",
    change: "modified",
    additions: 4,
    deletions: 0,
    binary: false,
    patch: TEST_PATCH,
    truncated: false,
  },
  {
    path: "src/checkout/redirect.ts",
    change: "modified",
    additions: 3,
    deletions: 1,
    binary: false,
    patch: CHECKOUT_PATCH,
    truncated: false,
  },
];

export const TEST_OUTPUT = [
  "> shop@1.4.0 test",
  "> vitest run src/checkout",
  "",
  " ✓ src/checkout/redirect.test.ts (4 tests) 12ms",
  " ✓ src/checkout/cart.test.ts (9 tests) 31ms",
  "",
  " Test Files  2 passed (2)",
  "      Tests  13 passed (13)",
].join("\n");

const usage = (input: number, output: number, fill: number): ThreadEvent => ({
  type: "usage",
  input,
  output,
  cacheRead: 0,
  cacheWrite: 0,
  contextFillPct: fill,
  costUsd: null,
});

/** A turn that reads, edits, asks to run the tests, and reports back. The
 * first turn of the demo, whatever the visitor typed. */
export function fixTurn(approvalId: string, expiresAt: number): Step[] {
  return [
    {
      wait: 500,
      event: {
        type: "reasoning",
        text: "The loop starts when a guest lands on /login with next=/login: the guard sends them back to the login page it came from.",
        durationMs: 1800,
      },
    },
    { wait: 600, event: tool("t1", "grep", "nextStep src/", "running") },
    { wait: 500, event: tool("t1", "grep", "nextStep src/", "ok", { output: "src/checkout/redirect.ts:12\nsrc/checkout/redirect.test.ts:3" }) },
    { wait: 300, event: tool("t2", "read", "src/checkout/redirect.ts", "running") },
    { wait: 500, event: tool("t2", "read", "src/checkout/redirect.ts", "ok", { output: "(24 lines)" }) },
    { wait: 700, event: tool("t3", "edit", "src/checkout/redirect.ts", "running") },
    { wait: 500, event: tool("t3", "edit", "src/checkout/redirect.ts", "ok", { diff: CHECKOUT_PATCH }) },
    { wait: 400, event: tool("t4", "edit", "src/checkout/redirect.test.ts", "running") },
    { wait: 500, event: tool("t4", "edit", "src/checkout/redirect.test.ts", "ok", { diff: TEST_PATCH }) },
    {
      approval: {
        type: "approval_request",
        id: approvalId,
        kind: "command",
        summary: "Run `npm test -- src/checkout`",
        detail: "npm test -- src/checkout",
        options: ["once", "thread"],
        expiresAt,
      },
    },
  ];
}

/** After Allow: the tests run and the agent wraps up. */
export function afterAllow(): Step[] {
  return [
    { wait: 300, event: tool("t5", "bash", "npm test -- src/checkout", "running") },
    { wait: 1400, event: tool("t5", "bash", "npm test -- src/checkout", "ok", { output: TEST_OUTPUT }) },
    {
      wait: 600,
      event: {
        type: "assistant",
        markdown: [
          "Fixed the redirect loop.",
          "",
          "- **Cause:** a guest on `/login?next=/login` was sent back to `/login`, forever.",
          "- **Fix:** `nextStep` now sends them to `/checkout` instead (`src/checkout/redirect.ts`).",
          "- **Test:** added *never redirects the login page to itself*; all 13 checkout tests pass.",
          "",
          "Open **Changes** to review the diff.",
        ].join("\n"),
      },
    },
    { wait: 100, event: usage(18_420, 1_260, 9.2) },
  ];
}

/** After Deny: nothing runs. */
export function afterDeny(): Step[] {
  return [
    {
      wait: 500,
      event: {
        type: "assistant",
        markdown:
          "Understood, I won't run the tests. The fix is in `src/checkout/redirect.ts` with a new test beside it; run `npm test -- src/checkout` when you're ready.",
      },
    },
    { wait: 100, event: usage(16_900, 640, 8.4) },
  ];
}

/** Any later message: a short streamed answer. */
export function chatTurn(text: string): Step[] {
  const lower = text.toLowerCase();
  const reply = lower.includes("plan")
    ? "Here's a plan:\n\n1. Reproduce the bug with a failing test.\n2. Fix the smallest piece of code that causes it.\n3. Run the related tests, then the whole suite.\n\nSwitch the composer to **Act** when you want me to start."
    : lower.includes("test")
      ? "In the real app I'd run that in your project, asking you first if the command isn't allowed yet. In this demo the agent is simulated, so nothing runs."
      : `This is a simulated agent, so I can't really work on "${text.slice(0, 80)}". In Vibezeo the message goes to Claude Code, Codex or another agent you've installed, running on your own machine and your own plan.\n\nDownload the app for Linux to try it on your code.`;
  return [
    { wait: 400, event: { type: "reasoning", text: "Answering the question.", durationMs: 600 } },
    { wait: 500, event: { type: "assistant", markdown: reply } },
    { wait: 100, event: usage(4_200 + text.length * 3, 180, 3.1) },
  ];
}

/** A Plan-mode answer: numbered steps for what was asked (F-TH-10). */
export function planText(text: string): string {
  const what = text.trim().replace(/\s+/g, " ").slice(0, 80) || "the change";
  return [
    `1. Find the code behind "${what}" and the tests that cover it.`,
    "2. Write a failing test for the new behaviour.",
    "3. Make the smallest change that passes it.",
    "4. Run the related tests, then the whole suite.",
  ].join("\n");
}

export function planTurn(plan: string): Step[] {
  return [
    { wait: 400, event: { type: "reasoning", text: "Planning only: nothing is edited in Plan mode.", durationMs: 700 } },
    { wait: 500, event: { type: "assistant", markdown: plan } },
    { wait: 100, event: usage(5_100, 220, 3.4) },
  ];
}
