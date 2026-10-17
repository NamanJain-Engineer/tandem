import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { LibrarySession } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { useHandoff } from "../../stores/handoff";
import { groups, useLibrary } from "../../stores/library";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { HandoffDialog } from "../code/HandoffDialog";
import { SessionSearchControl } from "../settings/controls";
import { SessionLibrary } from "./SessionLibrary";

const HOUR = 3_600_000;

function session(over: Partial<LibrarySession>): LibrarySession {
  return {
    key: "/k",
    engineId: "claude",
    accountId: "work",
    accountLabel: "Work",
    sessionId: "s",
    folder: "/src/shop",
    repository: "/src/shop",
    title: "A session",
    size: 2048,
    modifiedAt: Date.now() - HOUR,
    folderExists: true,
    ...over,
  };
}

const SESSIONS = [
  session({ key: "/a", sessionId: "aaaa-1111", title: "Fix the checkout total rounding" }),
  session({
    key: "/b",
    sessionId: "bbbb-2222",
    folder: "/src/shop/api",
    title: "Add a health endpoint",
    modifiedAt: Date.now() - 2 * HOUR,
  }),
  session({
    key: "/c",
    sessionId: "cccc-3333",
    accountId: "home",
    accountLabel: "Home",
    folder: "/src/blog",
    repository: "/src/blog",
    title: "Write the release notes",
    modifiedAt: Date.now() - 3 * HOUR,
  }),
  session({
    key: "/d",
    sessionId: "dddd-4444",
    engineId: "codex",
    accountId: null,
    accountLabel: null,
    title: "Speed up the product search",
    modifiedAt: Date.now() - 4 * HOUR,
  }),
];

const engine = (id: string, label: string) => ({
  spec: {
    id,
    label,
    vendor: "x",
    logo: label.slice(0, 2),
    commands: [id],
    newlineSequence: "\r",
    note: null,
  },
  install: { path: `/bin/${id}`, version: "1.0.0", source: "shell" },
  chatCapable: true,
  unsupportedReason: null,
  extraArgs: [],
});

let calls: Call[];
function open(handlers: Record<string, (a?: Record<string, unknown>) => unknown> = {}) {
  calls = fakeBackend({
    engines_list: () => [engine("claude", "Claude Code"), engine("codex", "Codex")],
    engines_report: () => null,
    library_list: () => ({ sessions: SESSIONS, skipped: 1 }),
    ...handlers,
  });
  useLibrary.getState().show();
  render(
    <UiProvider>
      <SessionLibrary />
      <HandoffDialog />
    </UiProvider>,
  );
}

const pro = () =>
  useSession.setState({
    entitlement: { ...FREE_VIEW, plan: "pro", limits: { ...FREE_VIEW.limits, sessionSearch: true } },
  });

describe("UI-162 session library (F-TH-17)", () => {
  beforeEach(() => {
    resetStores();
    useLibrary.getState().hide();
    useHandoff.getState().close();
  });

  it("F-TH-17.1 lists four sessions under their repositories with engine, account and date, and counts a skipped file", async () => {
    open();
    const rows = await screen.findAllByTestId("library-row");
    expect(rows).toHaveLength(4);
    const heads = screen
      .getAllByTestId("library-group")
      .map((g) => within(g).getByRole("heading").textContent);
    expect(heads).toEqual(["shop", "blog"]);
    const codex = rows.find((r) => r.dataset.sessionId === "dddd-4444")!;
    expect(codex.textContent).toContain("Codex · Outside Vibezeo · 4h");
    expect(rows[0]!.textContent).toContain("Claude Code · Work · 1h · 2.0 KB");
    expect(screen.getByTestId("library-skipped").textContent).toBe("1 files couldn't be read");
    // The filter narrows by title or folder, on this computer.
    fireEvent.change(screen.getByTestId("library-filter"), { target: { value: "release" } });
    expect(screen.getAllByTestId("library-row")).toHaveLength(1);
  });

  it("F-TH-17.1 Open shows the transcript read-only, tools by name", async () => {
    open({
      library_transcript: () => [
        { role: "user", text: "Fix the rounding" },
        { role: "tool", text: "Edit" },
        { role: "assistant", text: "Fixed." },
      ],
    });
    const rows = await screen.findAllByTestId("library-row");
    fireEvent.click(within(rows[0]!).getByRole("button", { name: "Open" }));
    const log = await screen.findByRole("log");
    expect(log.textContent).toContain("Fix the rounding");
    expect(log.textContent).toContain("Used Edit");
    expect(calls.find((c) => c.cmd === "library_transcript")?.args).toEqual({ key: "/a" });
  });

  it("F-TH-17.2 Resume asks the core to resume that session, and a gone folder can't resume", async () => {
    open({
      library_list: () => ({
        sessions: [SESSIONS[0]!, session({ key: "/gone", sessionId: "gone", folderExists: false })],
        skipped: 0,
      }),
      library_resume: () => {
        throw { code: "limit_reached", message: "Free includes 4 live panes" };
      },
    });
    const rows = await screen.findAllByTestId("library-row");
    expect((within(rows[1]!).getByTestId("library-resume") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => fireEvent.click(within(rows[0]!).getByTestId("library-resume")));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === "library_resume" && c.args?.key === "/a")).toBe(true),
    );
    expect(await screen.findByText("Free includes 4 live panes")).toBeTruthy();
  });

  it("F-TH-17.3 Continue with another engine opens the handoff sheet with a brief from the transcript", async () => {
    open({
      library_brief: () => ({ text: "## Goal\nFix the rounding", source: "fallback" }),
      library_continue: () => {
        throw { code: "engine_missing", message: "Codex isn't installed" };
      },
    });
    const rows = await screen.findAllByTestId("library-row");
    await act(async () => fireEvent.click(within(rows[0]!).getByTestId("library-continue")));
    const summary = (await screen.findByTestId("handoff-summary")) as HTMLTextAreaElement;
    await waitFor(() => expect(summary.value).toContain("Fix the rounding"));
    expect(calls.find((c) => c.cmd === "library_brief")?.args).toEqual({ key: "/a" });
    expect(calls.some((c) => c.cmd === "handoff_brief")).toBe(false);
    expect(screen.queryByText("Close the original pane after handoff")).toBeNull();
    await act(async () => fireEvent.click(screen.getByTestId("handoff-start")));
    await waitFor(() => expect(calls.some((c) => c.cmd === "library_continue")).toBe(true));
    expect(calls.find((c) => c.cmd === "library_continue")?.args).toMatchObject({
      key: "/a",
      engineId: "codex",
    });
  });

  it("F-TH-17.4 on Free search says Pro; on Pro with search on a word finds its session", async () => {
    open({ library_search: () => ["/c"] });
    await screen.findAllByTestId("library-row");
    expect(screen.getByText("Searching what sessions say is in Vibezeo Pro.")).toBeTruthy();
    expect(screen.queryByTestId("library-search")).toBeNull();
    act(() => {
      pro();
      useSettings.setState({ values: { session_search: true }, loaded: true });
    });
    fireEvent.change(screen.getByTestId("library-filter"), { target: { value: "notes" } });
    await act(async () => fireEvent.click(screen.getByTestId("library-search")));
    await waitFor(() => expect(screen.getAllByTestId("library-row")).toHaveLength(1));
    expect(screen.getAllByTestId("library-row")[0]!.dataset.sessionId).toBe("cccc-3333");
    expect(calls.find((c) => c.cmd === "library_search")?.args).toEqual({ query: "notes" });
  });

  it("F-TH-17.4 the Privacy switch needs Pro to turn on, and turning it off goes through the core", async () => {
    calls = fakeBackend({ library_search_set: () => null });
    const { rerender } = render(
      <UiProvider>
        <SessionSearchControl ids={{ id: "s" }} label="Search session text" />
      </UiProvider>,
    );
    const sw = screen.getByRole("switch", { name: "Search session text" });
    expect((sw as HTMLButtonElement).disabled).toBe(true);
    act(() => {
      pro();
      useSettings.setState({ values: { session_search: true }, loaded: true });
    });
    rerender(
      <UiProvider>
        <SessionSearchControl ids={{ id: "s" }} label="Search session text" />
      </UiProvider>,
    );
    await act(async () => fireEvent.click(screen.getByRole("switch", { name: "Search session text" })));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "library_search_set")?.args).toEqual({ on: false }),
    );
    await waitFor(() => expect(useSettings.getState().values.session_search).toBe(false));
  });

  it("UI-162 loading, empty and error states", async () => {
    let answer: (v: unknown) => void = () => undefined;
    open({ library_list: () => new Promise((r) => (answer = r)) });
    expect(await screen.findByLabelText("Reading sessions…")).toBeTruthy();
    await act(async () => answer({ sessions: [], skipped: 2 }));
    expect(await screen.findByText("No sessions found")).toBeTruthy();
    expect(screen.getByTestId("library-skipped").textContent).toBe("2 files couldn't be read");
  });

  it("UI-162 an error offers Try again, which reads the homes again", async () => {
    let fail = true;
    open({
      library_list: () => {
        if (fail) throw { code: "io", message: "disk unreadable" };
        return { sessions: SESSIONS, skipped: 0 };
      },
    });
    expect((await screen.findByText(/Couldn't read the sessions: disk unreadable/)).textContent).toBeTruthy();
    fail = false;
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
    expect(await screen.findAllByTestId("library-row")).toHaveLength(4);
  });

  it("groups put sessions without a folder last", () => {
    const out = groups([session({ key: "/x", repository: null, folder: null }), session({ key: "/y" })]);
    expect(out.map((g) => g.repository)).toEqual(["/src/shop", null]);
  });
});
