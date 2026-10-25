import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { PluginCard, Teammate, TeammateSummary, Thread } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { followTeammates, useTeammates } from "../../stores/teammates";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { AgentRail } from "./AgentRail";
import { AgentMain } from "./TeammatePage";

function tm(id: string, name: string, extra: Partial<Teammate> = {}): Teammate {
  return {
    id,
    name,
    avatarHue: null,
    avatarImage: null,
    brief: `You help with ${name}.`,
    engineId: "claude",
    model: null,
    effort: null,
    preferredAccountId: null,
    home: `/data/teammates/${id}`,
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
    ...extra,
  };
}

function summary(t: Teammate, extra: Partial<TeammateSummary> = {}): TeammateSummary {
  return {
    teammate: t,
    chatCapable: true,
    working: 0,
    waiting: false,
    failed: false,
    lastLine: null,
    lastAt: null,
    chats: 0,
    ...extra,
  };
}

function chat(id: string, title: string, teammateId: string): Thread {
  return {
    id,
    kind: "teammate_chat",
    workspaceId: null,
    cwd: `/data/teammates/${teammateId}`,
    engineId: "claude",
    accountId: null,
    model: null,
    effort: null,
    access: "edits",
    mode: "act",
    title,
    state: "idle",
    attended: true,
    engineSessionId: null,
    pinned: false,
    archived: false,
    unread: false,
    sortKey: id,
    createdAt: 1,
    updatedAt: 1,
    teammateId,
    conversationRef: null,
  };
}

const LEAD = tm("tm_lead", "Lead", { isLead: true, pinned: true });
const SCOUT = tm("tm_scout", "Scout");
const RHEA = tm("tm_rhea", "Rhea");

describe("UI-70 Agent rail", () => {
  let list: TeammateSummary[];
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    list = [
      summary(LEAD, { lastLine: "Asked Rhea to check CI.", lastAt: Date.now() - 60_000 }),
      summary(SCOUT, { working: 2 }),
      summary(RHEA, { failed: true, waiting: true }),
    ];
    calls = fakeBackend({
      teammate_list: () => list,
      teammate_delete: () => null,
      teammate_update: (a) => ({ ...SCOUT, ...(a?.patch as object) }),
    });
    followTeammates();
    render(
      <UiProvider>
        <AgentRail collapsed={false} />
      </UiProvider>,
    );
  });

  it("F-AG-01.3 the pinned Lead is the hero tile, with its last line", async () => {
    const hero = await screen.findByTestId("teammate-hero");
    expect(hero.dataset.teammateId).toBe("tm_lead");
    expect(within(hero).getByTestId("teammate-line").textContent).toBe("Asked Rhea to check CI.");
    expect(screen.getAllByTestId("teammate-row").map((r) => r.dataset.teammateId)).toEqual([
      "tm_scout",
      "tm_rhea",
    ]);
  });

  it("F-AG-02 the trailing slot: Failed before Waiting before working", async () => {
    const rows = await screen.findAllByTestId("teammate-row");
    expect(within(rows[0] as HTMLElement).getByTestId("teammate-trailing").textContent).toBe("2 working");
    expect(within(rows[1] as HTMLElement).getByTestId("teammate-trailing").textContent).toBe("Failed");
  });

  it("F-AG-02.1 a pending approval shows Needs you as soon as the change arrives", async () => {
    await screen.findAllByTestId("teammate-row");
    act(() =>
      emitLocal("teammates:changed", { teammates: [list[0], summary(SCOUT, { waiting: true }), list[2]] }),
    );
    const scout = screen.getAllByTestId("teammate-row")[0] as HTMLElement;
    expect(within(scout).getByTestId("teammate-trailing").textContent).toBe("Needs you");
    expect(scout.dataset.status).toBe("waiting");
  });

  it("F-AG-02 Delete asks first, then deletes", async () => {
    const rows = await screen.findAllByTestId("teammate-row");
    fireEvent.contextMenu(rows[0] as HTMLElement);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete…" }));
    await act(async () => fireEvent.click(await screen.findByRole("button", { name: "Delete teammate" })));
    expect(calls.some((c) => c.cmd === "teammate_delete" && c.args?.id === "tm_scout")).toBe(true);
  });
});

describe("UI-71 teammate page", () => {
  let granted: string[];
  let calls: Call[];
  let chats: Thread[];
  beforeEach(() => {
    resetStores();
    granted = [];
    chats = [
      chat("th_a", "Weekly deps", "tm_scout"),
      chat("th_b", "Test flake", "tm_scout"),
      chat("th_c", "Upgrade", "tm_scout"),
    ];
    const cards: PluginCard[] = [
      {
        pluginId: "test_echo",
        label: "Test Echo",
        category: "dev",
        description: "",
        status: "ready",
        state: "connected",
        accountLabel: null,
        connectionId: "pc_1",
        logo: "TE",
        authKind: "none",
        methods: ["none"],
        unavailableReason: null,
        takesKey: false,
        keyHint: null,
        teammates: 0,
        limitReached: false,
      },
      {
        pluginId: "test_down",
        label: "Test Down",
        category: "dev",
        description: "",
        status: "ready",
        state: "available",
        accountLabel: null,
        connectionId: null,
        logo: "TD",
        authKind: "none",
        methods: ["none"],
        unavailableReason: null,
        takesKey: false,
        keyHint: null,
        teammates: 0,
        limitReached: false,
      },
    ];
    calls = fakeBackend({
      teammate_list: () => [summary(SCOUT)],
      thread_list: () => chats,
      thread_reorder: (a) => {
        const ids = a?.ids as string[];
        chats = ids.map((id) => chats.find((c) => c.id === id) as Thread);
        return null;
      },
      teammate_suggestions: () => [
        "Check for outdated packages",
        "Explain the build",
        "Suggest what we should do next",
      ],
      plugins_catalog: () => cards,
      grants_list: () => granted,
      grants_set: (a) => {
        granted.push(String(a?.connectionId));
        return null;
      },
      thread_get: () => {
        throw { code: "not_found", message: "no" };
      },
    });
    followTeammates();
    useTeammates.getState().select("tm_scout");
    render(
      <UiProvider>
        <AgentMain visible />
      </UiProvider>,
    );
  });

  it("F-AG-03.1 dragging a chat tab saves the new order", async () => {
    const tabs = await screen.findAllByTestId("chat-tab");
    expect(tabs.map((t) => t.dataset.chatId)).toEqual(["th_a", "th_b", "th_c"]);
    const data = new Map<string, string>();
    const dataTransfer = {
      setData: (k: string, v: string) => data.set(k, v),
      getData: (k: string) => data.get(k) ?? "",
      effectAllowed: "",
    };
    fireEvent.dragStart(tabs[0] as HTMLElement, { dataTransfer });
    fireEvent.drop(tabs[2] as HTMLElement, { dataTransfer });
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "thread_reorder")?.args?.ids).toEqual(["th_b", "th_c", "th_a"]),
    );
    expect(screen.getAllByTestId("chat-tab").map((t) => t.dataset.chatId)).toEqual(["th_b", "th_c", "th_a"]);
  });

  it("F-AG-03.2 the nudge names connected plugins that are off, and Enable grants them", async () => {
    const nudge = await screen.findByTestId("plugin-nudge");
    expect(nudge.textContent).toContain("Test Echo are connected but off for Scout");
    expect(nudge.textContent).not.toContain("Test Down");
    await act(async () => fireEvent.click(within(nudge).getByRole("button", { name: "Enable" })));
    expect(calls.some((c) => c.cmd === "grants_set" && c.args?.connectionId === "pc_1")).toBe(true);
    await waitFor(() => expect(screen.queryByTestId("plugin-nudge")).toBeNull());
  });

  it("F-AG-03 arrow keys move between chat tabs", async () => {
    const tabs = await screen.findAllByTestId("chat-tab");
    const last = tabs[2] as HTMLElement;
    expect(last.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(last, { key: "ArrowLeft" });
    await waitFor(() =>
      expect(screen.getAllByTestId("chat-tab")[1]?.getAttribute("aria-selected")).toBe("true"),
    );
  });
});
