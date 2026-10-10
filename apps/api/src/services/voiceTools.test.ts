import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  catalogJson,
  checkCall,
  decodeCallId,
  encodeCallId,
  SETTABLE_SETTINGS,
  toolDefinitions,
  VOICE_TOOLS,
} from "./voiceTools";

// PRODUCT_SPEC F-CN-02's table, group by group.
const F_CN_02_TABLE = [
  ...[
    "switch_mode",
    "open_settings",
    "toggle_sidebar",
    "open_sidebar_tab",
    "open_launch_wizard",
    "open_routines",
  ],
  "dismiss",
  ...["list_workspaces", "list_panes", "list_threads", "list_teammates", "get_waiting"],
  ...[
    "launch_agents",
    "prompt_pane",
    "prompt_panes",
    "read_pane",
    "wait_for_pane",
    "focus_pane",
    "close_pane",
  ],
  ...["tidy", "flip", "handoff", "run_command_in_shell"],
  ...["open_thread", "new_thread", "send_to_thread", "read_thread", "stop_thread", "set_thread_mode"],
  ...["rename_thread", "archive_thread"],
  ...["open_teammate", "message_teammate", "create_teammate", "pin_teammate", "read_teammate_memory"],
  ...["list_routines", "run_routine"],
  ...["create_task", "execute_task", "list_tasks"],
  "find_work",
  ...["get_setting", "set_setting", "get_usage", "get_credits", "check_updates"],
];

describe("F-CN-02 the doer's closed tool catalog", () => {
  it("F-CN-02 is exactly the F-CN-02 table, each tool typed and closed", () => {
    expect(VOICE_TOOLS.map((t) => t.name)).toEqual(F_CN_02_TABLE);
    const defs = toolDefinitions();
    expect(defs.map((d) => d.name)).toEqual(F_CN_02_TABLE);
    for (const def of defs) {
      expect(def.description.length).toBeGreaterThan(20);
      expect(def.input_schema).toMatchObject({ type: "object", additionalProperties: false });
    }
  });

  it("F-CN-03 local-review actions are absent: forced lane removal, Full access, plugin spend", () => {
    const names = VOICE_TOOLS.map((t) => t.name).join(" ");
    expect(names).not.toMatch(/lane|full|access|approv|spend|plugin|destructive|delete/);
    expect(SETTABLE_SETTINGS).not.toContain("default_access");
    expect(VOICE_TOOLS.filter((t) => t.confirm).map((t) => t.name)).toEqual([
      "close_pane",
      "run_command_in_shell",
      "create_teammate",
      "execute_task",
    ]);
  });

  it("F-CN-02.2 set_setting takes only the closed list", () => {
    expect([...SETTABLE_SETTINGS].sort()).toEqual(
      [
        "theme",
        "notify_system",
        "notify_sound",
        "dictate_engine",
        "dictate_mic",
        "pause_all_mail",
        "conductor_voice",
        "conductor_tone",
        "conductor_verbosity",
      ].sort(),
    );
    expect(checkCall("set_setting", { key: "theme", value: "dark" })).toEqual({
      ok: true,
      args: { key: "theme", value: "dark" },
    });
    expect(checkCall("set_setting", { key: "telemetry_usage", value: "on" }).ok).toBe(false);
    expect(checkCall("set_setting", { key: "theme", value: "dark", also: "x" }).ok).toBe(false);
    expect(checkCall("get_setting", { key: "default_access" }).ok).toBe(false);
  });

  it("F-CN-02 calls outside the catalog or its types are refused", () => {
    expect(checkCall("rm_rf", {}).ok).toBe(false);
    expect(checkCall("launch_agents", { engine: "claude", count: 99, workspace: "w1" }).ok).toBe(false);
    expect(checkCall("read_pane", { pane: "p1", lines: 201 }).ok).toBe(false);
    expect(checkCall("read_pane", { pane: "p1", lines: 50 })).toEqual({
      ok: true,
      args: { pane: "p1", lines: 50 },
    });
    expect(checkCall("prompt_panes", { panes: ["p1", "p2"], texts: ["one"] }).ok).toBe(false);
    expect(checkCall("list_panes", null).ok).toBe(false);
  });

  it("F-CN-02 a call id carries its round, tool and short arguments, never spoken text", () => {
    const id = encodeCallId(3, 1, "prompt_pane", {
      pane: "p1",
      text: "fix the walrus-ember-42 test",
      send: true,
    });
    expect(id.length).toBeLessThanOrEqual(200);
    expect(decodeCallId(id)).toEqual({
      round: 3,
      name: "prompt_pane",
      args: { pane: "p1", text: "[28 chars]", send: true },
    });
    const encoded = id.split(".").at(-1) ?? "";
    expect(Buffer.from(encoded, "base64url").toString("utf8")).not.toContain("walrus");
    expect(decodeCallId(encodeCallId(1, 0, "get_waiting", {}))).toEqual({
      round: 1,
      name: "get_waiting",
      args: {},
    });
    expect(decodeCallId("vc.x.y")).toBeNull();
    expect(decodeCallId("vc.2.0.not_a_tool")).toBeNull();
    expect(decodeCallId("toolu_01ABC")).toBeNull();
  });

  it("F-CN-02 voice-tools.json is the catalog, for the desktop's registry (run `pnpm --filter api openapi`)", () => {
    const committed = JSON.parse(readFileSync(new URL("../../voice-tools.json", import.meta.url), "utf8"));
    expect(committed).toEqual(catalogJson());
  });
});
