import { act, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Terminal, type ILink } from "@xterm/xterm";
import type { FileContent, ResolvedLink, WorkspaceSummary } from "@tandem/protocol";
import { useFiles } from "../../stores/files";
import { useSettings } from "../../stores/settings";
import { useSidebar } from "../../stores/sidebar";
import { useTerminals } from "../../stores/terminals";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import {
  openFileLink,
  opensLink,
  openUrl,
  pathCandidates,
  pathLinkProvider,
  resetLinks,
  resolveLink,
} from "./links";

const write = (term: Terminal, data: string) => new Promise<void>((resolve) => term.write(data, resolve));

const file = (target: string, line: number | null = null, col: number | null = null): ResolvedLink => ({
  kind: "file",
  target,
  line,
  col,
  readonly: false,
});

/** The links a provider gives for buffer line `y` (1-based). */
function linksOn(
  term: Terminal,
  y: number,
  resolve: (text: string) => Promise<ResolvedLink | null>,
  opened: ResolvedLink[] = [],
) {
  return new Promise<ILink[] | undefined>((done) =>
    pathLinkProvider(term, resolve, (_e, link) => opened.push(link)).provideLinks(y, done),
  );
}

const WS = {
  id: "ws_1",
  root: "/work/shop",
  name: "shop",
  tabs: [{ id: "tab_1" }],
} as unknown as WorkspaceSummary;

beforeEach(() => {
  resetStores();
  resetLinks();
});

afterEach(() => resetLinks());

describe("terminal links (F-CD-09)", () => {
  it("F-CD-09 paths programs print are candidates, with :line[:col] and without a sentence's punctuation; URLs aren't", () => {
    const texts = (line: string) => pathCandidates(line).map((c) => c.text);
    expect(pathCandidates("error: src/main.rs:42:7: expected `;`")).toEqual([
      { start: 7, text: "src/main.rs:42:7" },
    ]);
    expect(texts("see /etc/hosts, ./a/b.ts and (../c/d.ts:3).")).toEqual([
      "/etc/hosts",
      "./a/b.ts",
      "../c/d.ts:3",
    ]);
    expect(texts(String.raw`at C:\work\shop\a.rs:3:4`)).toEqual([String.raw`C:\work\shop\a.rs:3:4`]);
    expect(texts("docs at https://example.com/a/b.rs or file:///etc/hosts")).toEqual([]);
    expect(texts("plain words, main.rs and v1.2")).toEqual([]);
  });

  it("F-CD-09.1 src/main.rs:42:7 in a pane is a link over exactly its cells, also when it wraps", async () => {
    const term = new Terminal({ cols: 20, rows: 6, allowProposedApi: true });
    term.open(document.createElement("div"));
    await write(term, "at src/main.rs:42:7\r\nsee lib/a-long-folder/b.rs:9 ok");
    const asked: string[] = [];
    const resolve = async (text: string) => {
      asked.push(text);
      return text.startsWith("src/")
        ? file("/work/shop/src/main.rs", 42, 7)
        : text.startsWith("lib/")
          ? file("/work/shop/lib/a-long-folder/b.rs", 9)
          : null;
    };
    const opened: ResolvedLink[] = [];
    const [link] = (await linksOn(term, 1, resolve, opened)) ?? [];
    expect(link?.text).toBe("src/main.rs:42:7");
    expect(link?.range).toEqual({ start: { x: 4, y: 1 }, end: { x: 19, y: 1 } });
    link?.activate(new MouseEvent("click"), link.text);
    expect(opened).toEqual([file("/work/shop/src/main.rs", 42, 7)]);
    // A path wrapped onto the next row is one link on both rows.
    const second = await linksOn(term, 2, resolve);
    const third = await linksOn(term, 3, resolve);
    expect(second?.[0]?.range).toEqual({ start: { x: 5, y: 2 }, end: { x: 8, y: 3 } });
    expect(third?.[0]?.range).toEqual(second?.[0]?.range);
    expect(asked).toContain("lib/a-long-folder/b.rs:9");
    // A path the core doesn't find isn't a link.
    expect(await linksOn(term, 1, async () => null)).toBeUndefined();
    term.dispose();
  });

  it("F-CD-09 only Mod+Click opens, and never while the pane is hidden or a pane is dragged", () => {
    const pane = { mac: false, visible: true, dragging: false };
    expect(opensLink(new MouseEvent("click", { ctrlKey: true }), pane)).toBe(true);
    expect(opensLink(new MouseEvent("click"), pane)).toBe(false);
    expect(opensLink(new MouseEvent("click", { metaKey: true }), pane)).toBe(false);
    expect(opensLink(new MouseEvent("click", { metaKey: true }), { ...pane, mac: true })).toBe(true);
    expect(opensLink(new MouseEvent("click", { ctrlKey: true }), { ...pane, visible: false })).toBe(false);
    expect(opensLink(new MouseEvent("click", { ctrlKey: true }), { ...pane, dragging: true })).toBe(false);
  });

  it("F-CD-09.3 a link is resolved through the pane and again once its cwd changes", async () => {
    let cwd = "/work/shop";
    const calls = fakeBackend({ pane_resolve_link: (a) => file(`${cwd}/${String(a?.text)}`) });
    const put = (dir: string) => {
      cwd = dir;
      useTerminals
        .getState()
        .put({
          paneId: "pn_1",
          title: null,
          cwd: dir,
          altScreen: false,
          bracketedPaste: false,
          mouse: false,
          appCursor: false,
        });
    };
    put("/work/shop");
    expect((await resolveLink("pn_1", "src/main.rs"))?.target).toBe("/work/shop/src/main.rs");
    expect((await resolveLink("pn_1", "src/main.rs"))?.target).toBe("/work/shop/src/main.rs");
    expect(calls.filter((c) => c.cmd === "pane_resolve_link")).toHaveLength(1);
    // `cd sub`: the shell's OSC 7 changes the pane's cwd, and the link with it.
    put("/work/shop/sub");
    expect((await resolveLink("pn_1", "src/main.rs"))?.target).toBe("/work/shop/sub/src/main.rs");
    expect(calls.filter((c) => c.cmd === "pane_resolve_link").map((c) => c.args)).toEqual([
      { paneId: "pn_1", text: "src/main.rs" },
      { paneId: "pn_1", text: "src/main.rs" },
    ]);
  });

  it("F-CD-09 a URL opens in the app's browser, or the system browser when Open links in Tandem is off", async () => {
    const calls = fakeBackend({ app_open_external: () => null });
    openUrl("http://localhost:3000/");
    const tab = useSidebar.getState().tabs.find((t) => t.kind === "browser");
    expect(tab?.url).toBe("http://localhost:3000/");
    expect(useSidebar.getState().active).toBe(tab?.id);
    useSettings.setState((s) => ({ values: { ...s.values, open_links_in_tandem: false } }));
    openUrl("https://example.com/");
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "app_open_external")?.args).toEqual({ url: "https://example.com/" }),
    );
    expect(useSidebar.getState().tabs.filter((t) => t.kind === "browser")).toHaveLength(1);
  });

  it("F-CD-09.1 a file link opens the Files tab with the file at its line once the tab shows its folder", async () => {
    fakeBackend({
      workspace_list: () => [WS],
      editor_tabs_get: () => ({
        tabs: [],
        active: null,
        expanded: [],
        showIgnored: false,
        divider: null,
        view: "changes",
      }),
      editor_tabs_save: () => null,
      fs_watch: () => "fsw_1",
      fs_unwatch: () => null,
      fs_list: () => [],
      git_status: () => null,
      fs_read: (a) => {
        const path = String(a?.path).replace("/work/shop/", "");
        return {
          path,
          kind: "text",
          text: "fn main() {}\n",
          size: 13,
          readonly: false,
          readonlyReason: null,
          truncated: false,
          hash: "h",
          mtime: 1,
          dataUrl: null,
          hex: null,
        } satisfies FileContent;
      },
    });
    useWorkspaces.setState({ list: [WS], activeId: "ws_1" });
    expect(openFileLink(file("/work/shop/src/main.rs", 42, 7))).toBe(true);
    expect(useSidebar.getState().tabs.find((t) => t.id === useSidebar.getState().active)?.kind).toBe("files");
    // The Files tab mounts and takes its folder: the file opens there.
    await act(async () => {
      await useFiles.getState().setContext({ type: "workspace", workspaceId: "ws_1" });
    });
    await waitFor(() => expect(useFiles.getState().byKey["ws:ws_1"]?.active).toBe("src/main.rs"));
    const ctx = useFiles.getState().byKey["ws:ws_1"];
    expect(ctx?.view).toBe("files");
    expect(ctx?.tabs.find((f) => f.path === "src/main.rs")?.goto).toMatchObject({ line: 42, col: 7 });
    // With nothing showing in Files there's nowhere to open it.
    useWorkspaces.setState({ activeId: null });
    expect(openFileLink(file("/work/shop/src/main.rs"))).toBe(false);
  });
});
