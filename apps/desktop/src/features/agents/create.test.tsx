import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { EngineView, Teammate, TeammateSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import { useTeammates } from "../../stores/teammates";
import { FREE_VIEW, SIGNED_IN, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { NewTeammateSheet, useNewTeammate } from "./NewTeammateSheet";
import { TeammateSettings, patchOf } from "./TeammateSettings";

function engine(id: string, label: string, installed: boolean, chat = true): EngineView {
  return {
    spec: { id, label, logo: label.slice(0, 2) } as EngineView["spec"],
    install: installed
      ? ({ path: `/usr/bin/${id}`, version: "1", source: "shell" } as EngineView["install"])
      : null,
    chatCapable: chat,
  } as EngineView;
}

const SCOUT: Teammate = {
  id: "tm_scout",
  name: "Scout",
  avatarHue: null,
  avatarImage: null,
  brief: "You keep the changelog.",
  engineId: "claude",
  model: null,
  effort: null,
  preferredAccountId: null,
  home: "/data/teammates/tm_scout",
  places: [],
  settings: {
    memoryCap: 4096,
    learnMode: "ask",
    mayMessage: true,
    maySchedule: false,
    defaultAccess: "edits",
  },
  isLead: false,
  pinned: false,
  sortKey: "a",
  createdAt: 1,
};

describe("UI-72 New teammate sheet", () => {
  let calls: Call[];
  let installed: boolean;
  beforeEach(() => {
    resetStores();
    installed = false;
    useEngines.setState({ list: [engine("codex", "Codex", false)], loaded: true });
    calls = fakeBackend({
      teammate_templates: () => [{ id: "reviewer", name: "Reviewer", brief: "You review changes." }],
      engines_detect: () => {
        installed = true;
        return { found: [], shellsAsked: [], slowShell: null };
      },
      engines_list: () => [engine("codex", "Codex", installed)],
      teammate_create: (a) => ({ ...SCOUT, id: "tm_new", name: String((a?.input as { name: string }).name) }),
      teammate_list: () => [],
      teammate_draft_with_ai: () => ({
        name: "Release notes writer",
        brief: "You draft release notes.",
        engineId: "codex",
        pluginIds: [],
      }),
    });
    useNewTeammate.setState({ open: true });
    render(
      <UiProvider>
        <NewTeammateSheet />
      </UiProvider>,
    );
  });

  it("F-AG-04.1 Create is disabled with an uninstalled engine and enables after Recheck", async () => {
    fireEvent.change(await screen.findByTestId("teammate-name-input"), { target: { value: "Scout" } });
    const create = screen.getByTestId("teammate-create") as HTMLButtonElement;
    expect(create.disabled).toBe(true);
    expect(screen.getByText("Install Codex first, then Recheck.")).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByTestId("engines-recheck")));
    await waitFor(() =>
      expect((screen.getByTestId("teammate-create") as HTMLButtonElement).disabled).toBe(false),
    );
    await act(async () => fireEvent.click(screen.getByTestId("teammate-create")));
    const made = calls.find((c) => c.cmd === "teammate_create");
    expect((made?.args?.input as { name: string; engineId: string }).engineId).toBe("codex");
  });

  it("F-AG-04 offline or Free: templates instead of Create with AI", async () => {
    await screen.findByTestId("templates");
    expect(screen.queryByTestId("draft-card")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Reviewer" }));
    expect((screen.getByTestId("teammate-name-input") as HTMLInputElement).value).toBe("Reviewer");
  });
});

describe("UI-72 Create with AI", () => {
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    useSession.setState({ auth: SIGNED_IN, entitlement: { ...FREE_VIEW, plan: "pro" } });
    useEngines.setState({
      list: [engine("claude", "Claude Code", true), engine("codex", "Codex", true)],
      loaded: true,
    });
    calls = fakeBackend({
      teammate_list: () => [],
      teammate_draft_with_ai: () => ({
        name: "Release notes writer",
        brief: "You draft release notes.",
        engineId: "codex",
        pluginIds: [],
      }),
    });
    useNewTeammate.setState({ open: true });
    render(
      <UiProvider>
        <NewTeammateSheet />
      </UiProvider>,
    );
  });

  it("F-AG-04.2 the draft prefills the form and nothing is saved until Create", async () => {
    fireEvent.change(await screen.findByTestId("draft-description"), {
      target: { value: "Write our release notes" },
    });
    await act(async () => fireEvent.click(screen.getByTestId("draft-run")));
    await waitFor(() =>
      expect((screen.getByTestId("teammate-name-input") as HTMLInputElement).value).toBe(
        "Release notes writer",
      ),
    );
    expect((screen.getByTestId("teammate-brief") as HTMLTextAreaElement).value).toBe(
      "You draft release notes.",
    );
    expect(screen.getByRole("radio", { name: /Codex/ }).getAttribute("aria-checked")).toBe("true");
    expect(calls.some((c) => c.cmd === "teammate_create")).toBe(false);
    expect(screen.getByText("Nothing is saved until you press Create.")).toBeTruthy();
  });
});

describe("UI-75/76 teammate settings", () => {
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    useEngines.setState({
      list: [engine("claude", "Claude Code", true), engine("codex", "Codex", true)],
      loaded: true,
    });
    calls = fakeBackend({
      plugins_catalog: () => [],
      grants_list: () => [],
      models_list: () => [],
      teammate_update: (a) => ({ ...SCOUT, ...(a?.patch as object) }),
    });
    const s: TeammateSummary = {
      teammate: SCOUT,
      chatCapable: true,
      working: 0,
      waiting: false,
      failed: false,
      lastLine: null,
      lastAt: null,
      chats: 0,
    };
    useTeammates.setState({ list: [s], loaded: true, selectedId: SCOUT.id });
    render(
      <UiProvider>
        <TeammateSettings s={s} />
      </UiProvider>,
    );
  });

  it("F-AG-05.2 turning May message off saves it, through the Save bar", async () => {
    await screen.findByTestId("teammate-settings");
    expect(screen.queryByTestId("save-bar")).toBeNull();
    fireEvent.click(screen.getByRole("switch", { name: "May message teammates" }));
    await screen.findByTestId("save-bar");
    expect(useTeammates.getState().dirty).toBe(true);
    await act(async () => fireEvent.click(screen.getByTestId("settings-save")));
    const patch = calls.find((c) => c.cmd === "teammate_update")?.args?.patch as {
      settings: { mayMessage: boolean };
    };
    expect(patch.settings.mayMessage).toBe(false);
    await waitFor(() => expect(screen.queryByTestId("save-bar")).toBeNull());
  });

  it("F-AG-05 a patch carries only what changed", () => {
    const base = {
      name: "Scout",
      avatarHue: null,
      brief: "b",
      engineId: "claude",
      model: null,
      preferredAccountId: null,
      places: [],
      defaultAccess: "edits" as const,
      memoryCap: 4096,
      learnMode: "ask" as const,
      mayMessage: true,
      plugins: [],
    };
    expect(patchOf(SCOUT, base, { ...base, engineId: "codex" })).toEqual({ engineId: "codex" });
    expect(patchOf(SCOUT, base, base)).toEqual({});
  });
});
