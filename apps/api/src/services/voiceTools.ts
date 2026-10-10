// The Conductor doer's closed, typed tool catalog (PRODUCT_SPEC F-CN-02,
// ARCHITECTURE §12.6). The app runs each call through its ActionRegistry,
// which mirrors this list (`voice-tools.json`); calls outside it, or with
// arguments outside their types, never reach the app. Local-review actions
// (forced lane removal, Full access, plugin spend) are absent (F-CN-03).
import { z } from "zod";
import type { AiTool } from "../providers/ai/types";

/** `set_setting` keys Conductor may change (F-CN-02 AC2); everything else is refused. */
export const SETTABLE_SETTINGS = [
  "theme",
  "notify_system",
  "notify_sound",
  "dictate_engine",
  "dictate_mic",
  "pause_all_mail",
  "conductor_voice",
  "conductor_tone",
  "conductor_verbosity",
] as const;

const SETTINGS_PAGES = [
  "general",
  "appearance",
  "notifications",
  "terminal",
  "engines",
  "accounts",
  "teammates",
  "dictate",
  "conductor",
  "plugins",
  "remote",
  "privacy",
  "shortcuts",
  "account",
  "updates",
  "advanced",
] as const;

export type VoiceToolGroup =
  "navigation" | "awareness" | "panes" | "threads" | "teammates" | "tasks" | "find" | "app";

export type VoiceTool = {
  name: string;
  group: VoiceToolGroup;
  description: string;
  input: z.ZodObject;
  /** Arguments that carry what the user said: never copied into call ids. */
  freeText?: string[];
  /** The app asks for an on-screen click before it runs (F-CN-03). */
  confirm?: boolean;
};

const ref = (what: string) => z.string().min(1).max(200).describe(what);
const said = (what: string, max = 4000) => z.string().min(1).max(max).describe(what);
const none = z.strictObject({});

const PANE = "A pane id from the snapshot.";
const THREAD_OPEN = "A thread id from the snapshot; the open thread when left out.";

function tool(
  group: VoiceToolGroup,
  name: string,
  description: string,
  input: z.ZodObject,
  extra: Pick<VoiceTool, "freeText" | "confirm"> = {},
): VoiceTool {
  return { group, name, description, input, ...extra };
}

export const VOICE_TOOLS: VoiceTool[] = [
  // Navigation
  tool(
    "navigation",
    "switch_mode",
    "Switch the app to Agent, Code or Thread mode.",
    z.strictObject({ mode: z.enum(["agent", "code", "thread"]) }),
  ),
  tool(
    "navigation",
    "open_settings",
    "Open Settings, at a page when the user names one.",
    z.strictObject({ page: z.enum(SETTINGS_PAGES).optional() }),
  ),
  tool(
    "navigation",
    "toggle_sidebar",
    "Show or hide the left rail or the right sidebar.",
    z.strictObject({ side: z.enum(["left", "right"]) }),
  ),
  tool(
    "navigation",
    "open_sidebar_tab",
    "Open a tab of the right sidebar.",
    z.strictObject({ tab: z.enum(["dashboard", "browser", "files", "tasks", "scratchpad"]) }),
  ),
  tool(
    "navigation",
    "open_launch_wizard",
    "Open the launch wizard, for when the user wants to pick the agents and options on screen.",
    z.strictObject({ workspace: ref("A workspace id from the snapshot.").optional() }),
  ),
  tool(
    "navigation",
    "open_routines",
    "Open the routines view, where scheduled teammate runs are listed.",
    none,
  ),
  tool("navigation", "dismiss", "Close the dialog, sheet or menu that is open on screen.", none),
  // Awareness
  tool(
    "awareness",
    "list_workspaces",
    "List the workspaces (folders) with their ids, when the snapshot doesn't say enough.",
    none,
  ),
  tool(
    "awareness",
    "list_panes",
    "List panes with their ids, names, engines and statuses, in one workspace or all of them.",
    z.strictObject({ workspace: ref("A workspace id; every workspace when left out.").optional() }),
  ),
  tool("awareness", "list_threads", "List threads with their ids, titles and statuses.", none),
  tool("awareness", "list_teammates", "List teammates with their ids and names.", none),
  tool(
    "awareness",
    "get_waiting",
    "List everything waiting for the user: approvals, questions and finished work. Call it when the user asks what needs them.",
    none,
  ),
  // Panes and agents
  tool(
    "panes",
    "launch_agents",
    "Start coding agents in new panes of a workspace, optionally with a first task each.",
    z.strictObject({
      engine: ref("An engine id, such as claude or codex."),
      count: z.number().int().min(1).max(8),
      workspace: ref("A workspace id from the snapshot."),
      task: said("The first task, as a clear instruction.", 2000).optional(),
      isolation: z
        .enum(["shared", "worktree"])
        .optional()
        .describe("worktree gives each agent its own branch."),
    }),
    { freeText: ["task"] },
  ),
  tool(
    "panes",
    "prompt_pane",
    "Type a prompt into an agent's pane. send presses Enter. The app refuses a pane waiting for approval; tell the user then.",
    z.strictObject({ pane: ref(PANE), text: said("The prompt."), send: z.boolean() }),
    { freeText: ["text"] },
  ),
  tool(
    "panes",
    "prompt_panes",
    "Type a prompt into each of several agents' panes: texts[i] goes to panes[i], and each is sent.",
    z
      .strictObject({
        panes: z.array(ref(PANE)).min(1).max(8),
        texts: z.array(said("The prompt.")).min(1).max(8),
      })
      .refine((v) => v.panes.length === v.texts.length, { message: "One text per pane." }),
    { freeText: ["texts"] },
  ),
  tool(
    "panes",
    "read_pane",
    "Read the last lines of a pane's terminal; the app redacts secrets first.",
    z.strictObject({ pane: ref(PANE), lines: z.number().int().min(1).max(200).optional() }),
  ),
  tool(
    "panes",
    "wait_for_pane",
    "Wait until the agent in a pane finishes or needs the user, for at most timeout seconds.",
    z.strictObject({ pane: ref(PANE), timeout: z.number().int().min(1).max(300) }),
  ),
  tool("panes", "focus_pane", "Bring a pane into view and focus it.", z.strictObject({ pane: ref(PANE) })),
  tool(
    "panes",
    "close_pane",
    "Close a pane. The app asks the user to confirm on screen when its agent is working.",
    z.strictObject({ pane: ref(PANE) }),
    { confirm: true },
  ),
  tool(
    "panes",
    "tidy",
    "Auto-arrange a workspace's panes into an even grid.",
    z.strictObject({ workspace: ref("A workspace id; the current one when left out.").optional() }),
  ),
  tool(
    "panes",
    "flip",
    "Toggle Card view for the current workspace: cards instead of terminals, and back.",
    none,
  ),
  tool(
    "panes",
    "handoff",
    "Hand a pane's work to another engine, with a brief of what was done.",
    z.strictObject({ pane: ref(PANE), target: ref("The engine id to hand the work to.") }),
  ),
  tool(
    "panes",
    "run_command_in_shell",
    "Run a shell command in a pane's terminal, only when the user asked for that command. The user always confirms it on screen first.",
    z.strictObject({ pane: ref(PANE), cmd: said("The command.", 1000) }),
    { freeText: ["cmd"], confirm: true },
  ),
  // Threads
  tool(
    "threads",
    "open_thread",
    "Open a thread in Thread mode to show it to the user.",
    z.strictObject({ thread: ref("A thread id from the snapshot.") }),
  ),
  tool(
    "threads",
    "new_thread",
    "Start a new thread, optionally in a folder, with an engine and a first message.",
    z.strictObject({
      folder: said("The folder's path, from the snapshot.", 500).optional(),
      engine: ref("An engine id.").optional(),
      text: said("The first message, as a clear instruction.").optional(),
    }),
    { freeText: ["folder", "text"] },
  ),
  tool(
    "threads",
    "send_to_thread",
    "Send a message to a thread. send false leaves it in the composer for the user to review.",
    z.strictObject({ thread: ref(THREAD_OPEN).optional(), text: said("The message."), send: z.boolean() }),
    { freeText: ["text"] },
  ),
  tool(
    "threads",
    "read_thread",
    "Read the last messages of a thread; the app redacts secrets first.",
    z.strictObject({ thread: ref(THREAD_OPEN).optional(), last: z.number().int().min(1).max(50).optional() }),
  ),
  tool(
    "threads",
    "stop_thread",
    "Stop the turn a thread is running.",
    z.strictObject({ thread: ref(THREAD_OPEN).optional() }),
  ),
  tool(
    "threads",
    "set_thread_mode",
    "Switch a thread between Plan (it proposes) and Act (it works).",
    z.strictObject({ thread: ref(THREAD_OPEN).optional(), mode: z.enum(["plan", "act"]) }),
  ),
  tool(
    "threads",
    "rename_thread",
    "Give a thread the title the user asks for.",
    z.strictObject({ thread: ref(THREAD_OPEN).optional(), title: said("The new title.", 200) }),
    { freeText: ["title"] },
  ),
  tool(
    "threads",
    "archive_thread",
    "Archive a thread the user is done with: it leaves the list and isn't deleted.",
    z.strictObject({ thread: ref("A thread id.") }),
  ),
  // Teammates
  tool(
    "teammates",
    "open_teammate",
    "Open a teammate's page: its brief, memory and runs.",
    z.strictObject({ teammate: ref("A teammate id from the snapshot.") }),
  ),
  tool(
    "teammates",
    "message_teammate",
    "Send a message to a teammate, as the user would from its page.",
    z.strictObject({ teammate: ref("A teammate id."), text: said("The message.") }),
    { freeText: ["text"] },
  ),
  tool(
    "teammates",
    "create_teammate",
    "Draft a new teammate from a job description. The user confirms on screen before it is saved.",
    z.strictObject({ description: said("What the teammate does.", 2000) }),
    { freeText: ["description"], confirm: true },
  ),
  tool(
    "teammates",
    "pin_teammate",
    "Pin a teammate to the rail, or unpin it with pinned false.",
    z.strictObject({ teammate: ref("A teammate id."), pinned: z.boolean().optional() }),
  ),
  tool(
    "teammates",
    "read_teammate_memory",
    "Read what a teammate remembers.",
    z.strictObject({ teammate: ref("A teammate id.") }),
  ),
  tool("teammates", "list_routines", "List routines with their ids and schedules.", none),
  tool(
    "teammates",
    "run_routine",
    "Run a routine now, outside its schedule.",
    z.strictObject({ routine: ref("A routine id.") }),
  ),
  // Tasks
  tool(
    "tasks",
    "create_task",
    "Create a task with a title and, when the user gives them, details.",
    z.strictObject({ title: said("The title.", 200), description: said("The details.").optional() }),
    { freeText: ["title", "description"] },
  ),
  tool(
    "tasks",
    "execute_task",
    "Start agents on a task. The user confirms on screen first.",
    z.strictObject({
      task: ref("A task id."),
      engine: ref("An engine id.").optional(),
      workspace: ref("A workspace id.").optional(),
    }),
    { confirm: true },
  ),
  tool("tasks", "list_tasks", "List tasks with their ids, titles and statuses.", none),
  // Find
  tool(
    "find",
    "find_work",
    "Find the pane, thread or teammate the user describes by what it is about, and show it.",
    z.strictObject({ query: said("What the user is looking for.", 500) }),
    { freeText: ["query"] },
  ),
  // App
  tool(
    "app",
    "get_setting",
    "Read one of the settings Conductor may change.",
    z.strictObject({ key: z.enum(SETTABLE_SETTINGS) }),
  ),
  tool(
    "app",
    "set_setting",
    "Change one setting from a closed list: theme (light, dark, system), notify_system and notify_sound (true or false), dictate_engine (local or cloud), dictate_mic (a microphone's name), pause_all_mail (true or false), conductor_voice, conductor_tone (neutral, warm, brief) and conductor_verbosity (short, normal, detailed).",
    z.strictObject({
      key: z.enum(SETTABLE_SETTINGS),
      value: z.union([z.string().min(1).max(100), z.boolean()]),
    }),
  ),
  tool("app", "get_usage", "Read the engines' usage and limits.", none),
  tool("app", "get_credits", "Read the credit balance that pays for Conductor.", none),
  tool("app", "check_updates", "Check whether a newer version of the app is available.", none),
];

const BY_NAME = new Map(VOICE_TOOLS.map((t) => [t.name, t]));

function jsonSchema(schema: z.ZodObject): Record<string, unknown> {
  const { $schema: _, ...rest } = z.toJSONSchema(schema, { io: "input" }) as Record<string, unknown>;
  return rest;
}

let definitions: AiTool[] | null = null;

/** The catalog as Messages API tools: the same list every call, so it caches. */
export function toolDefinitions(): AiTool[] {
  definitions ??= VOICE_TOOLS.map((t) => ({
    name: t.name,
    description: t.confirm ? `${t.description} The app asks the user to confirm on screen.` : t.description,
    input_schema: jsonSchema(t.input),
  }));
  return definitions;
}

export type Checked = { ok: true; args: Record<string, unknown> } | { ok: false };

/** A model's call held to the catalog: a known tool with arguments of its type. */
export function checkCall(name: string, input: unknown): Checked {
  const tool = BY_NAME.get(name);
  if (!tool) return { ok: false };
  const parsed = tool.input.safeParse(input);
  return parsed.success ? { ok: true, args: parsed.data as Record<string, unknown> } : { ok: false };
}

const ID = /^vc\.([1-9]\d?)\.(\d{1,2})\.([a-z_]+)(?:\.([A-Za-z0-9_-]+))?$/;
/** Longer short-argument lists are left out of the id. */
const MAX_ID_ARGS = 120;

/**
 * A call id the app hands back with its result: `vc.<round>.<index>.<tool>`
 * plus the call's short arguments (ids, numbers, choices) in base64url. Free
 * text is replaced by its length, so an id never carries what the user said.
 * The doer stays stateless: the next round rebuilds its history from these.
 */
export function encodeCallId(
  round: number,
  index: number,
  name: string,
  args: Record<string, unknown>,
): string {
  const free = new Set(BY_NAME.get(name)?.freeText ?? []);
  const short: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(args)) {
    short[k] = !free.has(k) ? v : Array.isArray(v) ? `[${v.length} texts]` : `[${String(v).length} chars]`;
  }
  const json = JSON.stringify(short);
  const tail =
    json === "{}" || json.length > MAX_ID_ARGS ? "" : `.${Buffer.from(json, "utf8").toString("base64url")}`;
  return `vc.${round}.${index}.${name}${tail}`;
}

export type DecodedCall = { round: number; name: string; args: Record<string, unknown> };

/** What an id says about its call, or null for an id this API didn't make. */
export function decodeCallId(id: string): DecodedCall | null {
  const m = ID.exec(id);
  const name = m?.[3];
  if (!m || !name || !BY_NAME.has(name)) return null;
  let args: Record<string, unknown> = {};
  if (m[4]) {
    try {
      const parsed: unknown = JSON.parse(Buffer.from(m[4], "base64url").toString("utf8"));
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
      args = parsed as Record<string, unknown>;
    } catch {
      return null;
    }
  }
  return { round: Number(m[1]), name, args };
}

/** `apps/api/voice-tools.json`: the catalog the desktop's ActionRegistry is held to. */
export function catalogJson(): Record<string, unknown> {
  return {
    $comment:
      "Generated from apps/api/src/services/voiceTools.ts by `pnpm --filter api openapi`. The Conductor doer's closed tool catalog (PRODUCT_SPEC F-CN-02); the desktop's ActionRegistry maps every tool here to a command.",
    version: 1,
    settable_settings: [...SETTABLE_SETTINGS],
    tools: VOICE_TOOLS.map((t) => ({
      name: t.name,
      group: t.group,
      description: t.description,
      confirm: t.confirm ?? false,
      free_text: t.freeText ?? [],
      input_schema: jsonSchema(t.input),
    })),
  };
}
