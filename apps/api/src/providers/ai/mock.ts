/** The mock's fixed teammate draft (ARCHITECTURE §14.8). */
export const MOCK_TEAMMATE_DRAFT = {
  name: "Release Notes Writer",
  brief:
    "You draft the weekly changelog from merged pull requests. Verify every claim against the repository. Never publish without approval.",
  engine: "claude",
  plugins: ["github"],
} as const;

import {
  type AiProvider,
  type AiRequest,
  type AiResult,
  type AiToolCall,
  AiUnavailableError,
  tagged,
  taggedText,
} from "./types";

const COUNTS: Record<string, number> = {
  one: 1,
  two: 2,
  three: 3,
  four: 4,
  five: 5,
  six: 6,
  seven: 7,
  eight: 8,
};
const ENGINES = ["claude", "codex", "gemini"];

/** Lower-case words with a light stem, so "testing" and "tests" meet at "test". */
function terms(text: string): Set<string> {
  return new Set(
    text
      .toLowerCase()
      .split(/[^a-z0-9]+/)
      .filter(Boolean)
      .map((w) => w.replace(/(?:ing|ed|s)$/, "") || w),
  );
}

/** Find work (F-CN-05): the share of the query's words each candidate has. */
function rank(content: string): string {
  const query = terms(tagged(content, "query") ?? "");
  let candidates: Array<{ id?: unknown; text?: unknown }> = [];
  try {
    candidates = JSON.parse(tagged(content, "candidates") ?? "[]") as typeof candidates;
  } catch {
    candidates = [];
  }
  const matches = candidates
    .map((c) => {
      const words = terms(String(c.text ?? ""));
      const shared = [...query].filter((w) => words.has(w)).length;
      return {
        id: String(c.id ?? ""),
        score: query.size ? Math.round((shared / query.size) * 100) / 100 : 0,
      };
    })
    .filter((m) => m.score > 0)
    .sort((a, b) => b.score - a.score);
  return JSON.stringify({ matches });
}

function firstWorkspace(content: string): string | null {
  try {
    const snapshot = JSON.parse(tagged(content, "snapshot") ?? "{}") as {
      workspaces?: Array<{ id?: unknown }>;
    };
    const id = snapshot.workspaces?.[0]?.id;
    return typeof id === "string" ? id : null;
  } catch {
    return null;
  }
}

/**
 * The Conductor doer (F-CN-02) by keyword: an intent becomes one tool call;
 * once results come back, or on the last round, it answers "Done.".
 */
function doer(req: AiRequest, n: number): { text: string; toolCalls?: AiToolCall[] } {
  if (req.toolChoice === "none") return { text: "Done." };
  const content = req.messages.at(-1)?.content ?? "";
  const request = (tagged(content, "request") ?? "").toLowerCase();
  const call = (name: string, input: Record<string, unknown>) => ({
    text: "",
    toolCalls: [{ id: `toolu_mock_${n}`, name, input }],
  });
  if (request.includes("keep checking")) return call("get_waiting", {});
  if (tagged(content, "results") !== null) return { text: "Done." };
  if (/\b(launch|start)\b/.test(request)) {
    const word = Object.keys(COUNTS).find((w) => new RegExp(`\\b${w}\\b`).test(request));
    const digit = request.match(/\b([1-8])\b/)?.[1];
    return call("launch_agents", {
      engine: ENGINES.find((e) => request.includes(e)) ?? "claude",
      count: digit ? Number(digit) : word ? (COUNTS[word] ?? 1) : 1,
      workspace: firstWorkspace(content) ?? "current",
    });
  }
  if (request.includes("telemetry")) return call("set_setting", { key: "telemetry_usage", value: "on" });
  const theme = request.match(/\b(dark|light)\b/)?.[1];
  if (theme && request.includes("theme")) return call("set_setting", { key: "theme", value: theme });
  if (request.includes("waiting")) return call("get_waiting", {});
  const find = request.match(/\b(?:find|where is|where's)\s+(.+)/)?.[1];
  if (find) return call("find_work", { query: find });
  if (/\b(list|running)\b/.test(request)) return call("list_panes", {});
  return { text: "Sorry, I can't do that by voice yet." };
}

/**
 * Deterministic answers for dev and tests (ARCHITECTURE §14.8): a title is
 * the first 6 words title-cased; a rewrite is the input with whitespace
 * normalized; a teammate draft is `MOCK_TEAMMATE_DRAFT`; an Auto turn
 * prompts "Continue with the next step." until the scrollback shows
 * `ALL TESTS PASSED`, then says done; the voice doer maps keyword intents to
 * tool calls; find work scores by shared words.
 */
export class MockAi implements AiProvider {
  readonly name = "mock" as const;
  calls: AiRequest[] = [];
  /** Tests: make the next call fail. */
  failNext = false;

  async complete(req: AiRequest): Promise<AiResult> {
    this.calls.push(req);
    if (this.failNext) {
      this.failNext = false;
      throw new AiUnavailableError("mock failure", 503);
    }
    const last = req.messages.at(-1)?.content ?? "";
    if (req.feature === "voice_act") {
      const out = doer(req, this.calls.length);
      return {
        ...out,
        tokensIn: Math.ceil(last.length / 4),
        tokensOut: Math.ceil(out.text.length / 4) + (out.toolCalls ? 20 : 0),
        model: req.model,
        stopReason: out.toolCalls ? "tool_use" : "end_turn",
      };
    }
    const input = taggedText(last) ?? last;
    const text =
      req.feature === "voice_resolve"
        ? rank(last)
        : req.feature === "auto_next"
          ? JSON.stringify(
              input.includes("ALL TESTS PASSED")
                ? { action: "done", reason: "The tests pass." }
                : { action: "prompt", text: "Continue with the next step." },
            )
          : req.feature === "teammate_draft"
            ? JSON.stringify(MOCK_TEAMMATE_DRAFT)
            : req.feature === "title"
              ? input
                  .split(/\s+/)
                  .filter(Boolean)
                  .slice(0, 6)
                  .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
                  .join(" ")
              : input.replace(/\s+/g, " ").trim();
    return {
      text,
      tokensIn: Math.ceil(last.length / 4),
      tokensOut: Math.ceil(text.length / 4),
      model: req.model,
    };
  }
}
