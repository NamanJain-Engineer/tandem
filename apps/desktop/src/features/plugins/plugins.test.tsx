import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { ConnectProgress, HeldDraft, PluginApprovalDetail, PluginCard } from "@tandem/protocol";
import { ApprovalCard, UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { ConnectSheet } from "./ConnectSheet";
import { HeldActions } from "./HeldActions";
import { PluginApprovalFields } from "./PluginApproval";
import { PluginsPage } from "./PluginsPage";
import { ThreadPlugins } from "./ThreadPlugins";
import { filterCards, parseApproval, splitCards } from "./model";

function card(id: string, label: string, extra: Partial<PluginCard> = {}): PluginCard {
  return {
    pluginId: id,
    label,
    category: "dev",
    logo: label.slice(0, 2),
    description: `${label} things.`,
    status: "ready",
    state: "available",
    accountLabel: null,
    connectionId: null,
    authKind: "oauth_dcr",
    methods: ["oauth"],
    unavailableReason: null,
    takesKey: false,
    keyHint: null,
    teammates: 0,
    limitReached: false,
    ...extra,
  };
}

const GITHUB = card("github", "GitHub", {
  state: "connected",
  accountLabel: "octo-shop",
  connectionId: "pc_gh",
  teammates: 3,
  takesKey: true,
  methods: ["oauth", "key"],
});
const LINEAR = card("linear", "Linear", {
  methods: ["oauth", "key"],
  category: "business",
  takesKey: true,
  keyHint: "Create a key in Linear › Settings › API.",
});
const FIGMA = card("figma", "Figma", { status: "preview", category: "media" });
const STRIPE = card("stripe", "Stripe", { state: "blocked_by_org", category: "business" });

const ui = (node: React.ReactNode) => render(<UiProvider>{node}</UiProvider>);

describe("F-PL-03 UI-90 catalog", () => {
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    calls = fakeBackend({
      plugins_catalog: () => [GITHUB, LINEAR, FIGMA, STRIPE],
      held_drafts_list: () => [],
      grants_list: () => [],
    });
  });

  it("F-PL-03 lists connected plugins first with their account, then the rest", async () => {
    ui(<PluginsPage />);
    const gh = await screen.findByTestId("plugin-card-github");
    expect(gh.dataset.state).toBe("connected");
    expect(within(gh).getByText("octo-shop · 3 teammates")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Connected (1)" })).toBeTruthy();
    expect(screen.getByTestId("plugin-connect-linear").hasAttribute("disabled")).toBe(false);
    expect(screen.getByText(/Credentials stay in your system keychain/)).toBeTruthy();
  });

  it("F-PL-09 a preview module shows Preview and its Connect is disabled", async () => {
    ui(<PluginsPage />);
    const figma = await screen.findByTestId("plugin-card-figma");
    expect(within(figma).getByText("Preview")).toBeTruthy();
    expect(screen.getByTestId("plugin-connect-figma").hasAttribute("disabled")).toBe(true);
  });

  it("F-PL-05 a plugin off the Team allowlist can't be connected and says why", async () => {
    ui(<PluginsPage />);
    await screen.findByTestId("plugin-card-stripe");
    expect(screen.getByTestId("plugin-connect-stripe").hasAttribute("disabled")).toBe(true);
    expect(screen.getByTestId("plugin-why-stripe").textContent).toBe(
      "Your organization doesn't allow Stripe.",
    );
  });

  it("F-PL-09 a ready plugin this build can't connect says why and can't be connected (D-960)", async () => {
    fakeBackend({
      plugins_catalog: () => [
        card("gmail", "Gmail", {
          state: "unavailable",
          methods: [],
          unavailableReason: "Gmail needs an app registration that this build doesn't have yet.",
        }),
      ],
      held_drafts_list: () => [],
      grants_list: () => [],
    });
    ui(<PluginsPage />);
    const gmail = await screen.findByTestId("plugin-card-gmail");
    expect(within(gmail).getByText("Not available yet")).toBeTruthy();
    expect(screen.getByTestId("plugin-connect-gmail").hasAttribute("disabled")).toBe(true);
    expect(screen.getByTestId("plugin-why-gmail").textContent).toBe(
      "Gmail needs an app registration that this build doesn't have yet.",
    );
  });

  it("F-PL-03 search filters, and an empty search says so", async () => {
    ui(<PluginsPage />);
    await screen.findByTestId("plugin-card-github");
    fireEvent.change(screen.getByTestId("plugins-search"), { target: { value: "zzz" } });
    expect(screen.getByText("No plugins match “zzz”.")).toBeTruthy();
    fireEvent.change(screen.getByTestId("plugins-search"), { target: { value: "lin" } });
    expect(screen.getByTestId("plugin-card-linear")).toBeTruthy();
    expect(screen.queryByTestId("plugin-card-github")).toBeNull();
  });

  it("F-PL-03 Free's one connection shows Pro on the Connect buttons", async () => {
    calls = fakeBackend({
      plugins_catalog: () => [GITHUB, { ...LINEAR, limitReached: true }],
      held_drafts_list: () => [],
      grants_list: () => [],
    });
    ui(<PluginsPage />);
    const btn = await screen.findByTestId("plugin-connect-linear");
    expect(btn.textContent).toBe("Pro");
    expect(btn.hasAttribute("disabled")).toBe(true);
    expect(screen.getByTestId("plugin-why-linear").textContent).toBe("Free includes one plugin connection.");
    expect(calls.some((c) => c.cmd === "plugin_connect_oauth")).toBe(false);
  });

  it("F-PL-03 the model sorts connected then ready before preview", () => {
    const { connected, browse } = splitCards([FIGMA, LINEAR, GITHUB]);
    expect(connected.map((c) => c.pluginId)).toEqual(["github"]);
    expect(browse.map((c) => c.pluginId)).toEqual(["linear", "figma"]);
    expect(filterCards([GITHUB, LINEAR], "", "business").map((c) => c.pluginId)).toEqual(["linear"]);
  });
});

describe("F-PL-03 UI-91 connect sheet", () => {
  let calls: Call[];
  let keyResult: () => PluginCard | null;
  beforeEach(() => {
    resetStores();
    keyResult = () => ({ ...LINEAR, state: "connected", connectionId: "pc_li", accountLabel: "acme" });
    calls = fakeBackend({
      plugin_connect_oauth: () => {
        // A fast failure: the event arrives before the op id does.
        emitLocal<ConnectProgress>("plugins:connect", {
          opId: "op_1",
          pluginId: "github",
          step: "discovery",
          state: "failed",
          authorizeUrl: null,
          error: "The sign-in at http://169.254.169.254/ isn't one of GitHub's pinned hosts",
          card: null,
        });
        return { opId: "op_1" };
      },
      plugin_connect_cancel: () => null,
      plugin_connect_api_key: () => keyResult(),
      teammate_list: () => [],
      grants_set: () => null,
    });
  });

  it("F-PL-03 a failed step is named and the provider's text is shown untruncated", async () => {
    ui(<ConnectSheet card={card("github", "GitHub")} open onOpenChange={() => undefined} />);
    fireEvent.click(screen.getByTestId("connect-start"));
    const err = await screen.findByTestId("connect-error");
    expect(within(err).getByText("Couldn't connect: Find the sign-in failed.")).toBeTruthy();
    expect(err.textContent).toContain(
      "The sign-in at http://169.254.169.254/ isn't one of GitHub's pinned hosts",
    );
    expect(screen.getByTestId("step-discovery").dataset.state).toBe("failed");
  });

  it("F-PL-03 steps follow the op's progress and success shows the account", async () => {
    calls = fakeBackend({
      plugin_connect_oauth: () => ({ opId: "op_2" }),
      teammate_list: () => [],
    });
    ui(<ConnectSheet card={card("github", "GitHub")} open onOpenChange={() => undefined} />);
    fireEvent.click(screen.getByTestId("connect-start"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "plugin_connect_oauth")).toBe(true));
    await act(async () => {
      await Promise.resolve();
    });
    const step = (s: ConnectProgress["step"], state: ConnectProgress["state"], c: PluginCard | null = null) =>
      act(() =>
        emitLocal<ConnectProgress>("plugins:connect", {
          opId: "op_2",
          pluginId: "github",
          step: s,
          state,
          authorizeUrl: null,
          error: null,
          card: c,
        }),
      );
    step("discovery", "done");
    step("sign_in", "active");
    expect(screen.getByTestId("step-discovery").dataset.state).toBe("done");
    expect(screen.getByTestId("step-sign_in").dataset.state).toBe("active");
    expect(screen.getByText("waiting…")).toBeTruthy();
    // Another attempt's events don't move this one.
    act(() =>
      emitLocal<ConnectProgress>("plugins:connect", {
        opId: "op_other",
        pluginId: "github",
        step: "registration",
        state: "failed",
        authorizeUrl: null,
        error: "no",
        card: null,
      }),
    );
    expect(screen.queryByTestId("connect-error")).toBeNull();
    step("verification", "done", { ...GITHUB });
    expect(await screen.findByText("Connected as octo-shop")).toBeTruthy();
  });

  it("F-PL-03 an API key is masked, tested, and the teammates start unchecked", async () => {
    ui(<ConnectSheet card={LINEAR} open onOpenChange={() => undefined} />);
    fireEvent.click(screen.getByRole("radio", { name: "API key" }));
    const input = screen.getByTestId("connect-key-input") as HTMLInputElement;
    expect(input.type).toBe("password");
    expect(screen.getByText("Create a key in Linear › Settings › API.")).toBeTruthy();
    fireEvent.change(input, { target: { value: "lin_api_123" } });
    fireEvent.click(screen.getByTestId("connect-key"));
    expect(await screen.findByText("Connected as acme")).toBeTruthy();
    const sent = calls.find((c) => c.cmd === "plugin_connect_api_key");
    expect(sent?.args).toMatchObject({ pluginId: "linear", key: "lin_api_123", connectionId: null });
    fireEvent.click(screen.getByTestId("connect-finish"));
    expect(calls.some((c) => c.cmd === "grants_set")).toBe(false);
  });

  it("F-PL-03 a failed key verification shows the provider's words and saves nothing", async () => {
    keyResult = () => {
      throw { code: "invalid", message: "Linear said: 401 Authentication required, not authenticated" };
    };
    const closed: boolean[] = [];
    ui(<ConnectSheet card={LINEAR} reconnect="pc_li" open onOpenChange={(o) => closed.push(!o)} />);
    expect(screen.getByText(/Your current connection stays as it is/)).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "API key" }));
    fireEvent.change(screen.getByTestId("connect-key-input"), { target: { value: "bad" } });
    fireEvent.click(screen.getByTestId("connect-key"));
    const err = await screen.findByTestId("connect-error");
    expect(err.textContent).toContain("Linear said: 401 Authentication required, not authenticated");
    expect(closed).toEqual([]);
  });
});

const DETAIL: PluginApprovalDetail = {
  pluginId: "github",
  pluginLabel: "GitHub",
  logo: "Gh",
  accountLabel: "octo-shop",
  tool: "issue_write",
  action: "Create issue in octo-shop/web",
  target: "octo-shop/web",
  cost: null,
  riskClass: "write",
  request: JSON.stringify(
    { tool: "issue_write", arguments: { owner: "octo-shop", repo: "web", title: "Fix" } },
    null,
    2,
  ),
};

describe("F-PL-03 token-only plugins (D-961)", () => {
  it("F-PL-03 offers only the token, and says why, when the browser sign-in isn't in this build", () => {
    resetStores();
    fakeBackend({});
    const github = card("github", "GitHub", {
      authKind: "oauth_static",
      takesKey: true,
      methods: ["key"],
      unavailableReason: "GitHub's browser sign-in isn't supported in this build yet.",
      keyHint: "Create a fine-grained personal access token.",
    });
    ui(<ConnectSheet card={github} open onOpenChange={() => undefined} />);
    expect(screen.queryByRole("radio", { name: "Browser sign-in" })).toBeNull();
    expect(screen.queryByTestId("connect-start")).toBeNull();
    expect(screen.getByTestId("connect-key-input")).toBeTruthy();
    expect(screen.getByTestId("connect-why-token").textContent).toBe(
      "GitHub's browser sign-in isn't supported in this build yet.",
    );
  });
});

describe("F-PL-07 plugin approval card", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend({});
  });

  it("F-PL-07 renders every field from a fixture, and the full request holds no credential", () => {
    expect(parseApproval(JSON.stringify(DETAIL))).toEqual(DETAIL);
    expect(parseApproval("rm -rf build")).toBeNull();
    ui(<PluginApprovalFields detail={DETAIL} />);
    const box = screen.getByTestId("plugin-approval");
    for (const text of [
      "GitHub",
      "Write",
      "Create issue in octo-shop/web",
      "octo-shop",
      "octo-shop/web",
      "None",
    ]) {
      expect(within(box).getAllByText(text).length).toBeGreaterThan(0);
    }
    fireEvent.click(screen.getByRole("button", { name: "Full request" }));
    const req = screen.getByTestId("plugin-approval-request").textContent ?? "";
    expect(req).toContain('"issue_write"');
    expect(req).not.toMatch(/gho_|Bearer|access_token/);
  });

  it("F-PL-07 a spend card shows its cost in the danger tone and never offers the chat scope", () => {
    const spend = { ...DETAIL, riskClass: "spend" as const, cost: "5.00 USD", action: "Charge 5.00 USD" };
    ui(
      <ApprovalCard
        kind="spend"
        title="Wants to use Stripe"
        detail={<PluginApprovalFields detail={spend} />}
        allowThread
        onAllow={() => undefined}
        onAllowThread={() => undefined}
        onDeny={() => undefined}
      />,
    );
    expect(screen.getByTestId("plugin-approval-cost").className).toContain("text-error");
    expect(screen.getByTestId("plugin-approval-cost").textContent).toBe("5.00 USD");
    expect(screen.queryByTestId("approval-allow-thread")).toBeNull();
  });
});

function draft(n: number, extra: Partial<HeldDraft> = {}): HeldDraft {
  return {
    id: `hd_${n}`,
    displayNo: n,
    connectionId: "pc_gh",
    pluginId: "github",
    tool: "issue_write",
    args: { owner: "octo-shop", repo: "web", title: `Bump ${n}` },
    riskClass: "write",
    originKind: "routine",
    originId: "rt_nightly",
    runId: "run_1",
    threadId: null,
    state: "held",
    result: null,
    createdAt: 1,
    decidedAt: null,
    pluginLabel: "GitHub",
    logo: "Gh",
    action: "Create issue in octo-shop/web",
    schema: { type: "object", required: ["title"] },
    ...extra,
  };
}

describe("F-PL-08 held actions", () => {
  let calls: Call[];
  let drafts: HeldDraft[];
  beforeEach(() => {
    resetStores();
    drafts = [draft(12), draft(13)];
    calls = fakeBackend({
      held_drafts_list: () => drafts,
      held_draft_run: (a) => {
        if (a?.editedArgs && !(a.editedArgs as Record<string, unknown>).title)
          throw { code: "invalid", message: "title is required" };
        drafts = drafts.filter((d) => d.id !== a?.draftId);
        return { text: "Created issue #7", isError: false };
      },
      held_drafts_discard: (a) => {
        const ids = a?.draftIds as string[];
        drafts = drafts.filter((d) => !ids.includes(d.id));
        return null;
      },
    });
  });

  it("F-PL-08 lists each draft with where it came from", async () => {
    ui(<HeldActions />);
    const row = await screen.findByTestId("held-12");
    expect(within(row).getByText("D-12")).toBeTruthy();
    expect(within(row).getByText("GitHub · Create issue in octo-shop/web")).toBeTruthy();
    expect(within(row).getByText("Held · from routine rt_nightly")).toBeTruthy();
  });

  it("F-PL-08 Approve & run runs a draft once", async () => {
    ui(<HeldActions />);
    fireEvent.click(await screen.findByTestId("held-run-12"));
    await waitFor(() => expect(screen.queryByTestId("held-12")).toBeNull());
    expect(calls.filter((c) => c.cmd === "held_draft_run")).toHaveLength(1);
    expect(calls.find((c) => c.cmd === "held_draft_run")?.args).toMatchObject({
      draftId: "hd_12",
      editedArgs: null,
    });
  });

  it("F-PL-08 discards in bulk", async () => {
    ui(<HeldActions />);
    await screen.findByTestId("held-12");
    fireEvent.click(screen.getByRole("checkbox", { name: "Select D-12" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Select D-13" }));
    fireEvent.click(screen.getByTestId("held-discard-selected"));
    await waitFor(() => expect(screen.getByText("Nothing held")).toBeTruthy());
    expect(calls.find((c) => c.cmd === "held_drafts_discard")?.args).toEqual({
      draftIds: ["hd_12", "hd_13"],
    });
  });
});

describe("F-PL-05 plugins for this thread (F-TH-07)", () => {
  let calls: Call[];
  let threadGrants: string[];
  beforeEach(() => {
    resetStores();
    threadGrants = [];
    const SENTRY = card("sentry", "Sentry", {
      state: "connected",
      connectionId: "pc_se",
      accountLabel: "shop",
    });
    calls = fakeBackend({
      plugins_catalog: () => [GITHUB, SENTRY, LINEAR],
      grants_list: (a) => {
        const subject = a?.subject as { type: string };
        return subject.type === "all_threads" ? ["pc_se"] : threadGrants;
      },
      grants_set: (a) => {
        if (a?.granted) threadGrants.push(String(a.connectionId));
        return null;
      },
    });
  });

  it("F-PL-05 a thread starts with no plugins, granting one is per thread, and all-thread grants show locked", async () => {
    ui(<ThreadPlugins threadId="th_1" open onOpenChange={() => undefined} />);
    const gh = await screen.findByTestId("thread-plugin-github");
    const ghSwitch = within(gh).getByRole("switch");
    expect(ghSwitch.getAttribute("aria-checked")).toBe("false");
    expect(screen.queryByTestId("thread-plugin-linear")).toBeNull();
    const se = within(screen.getByTestId("thread-plugin-sentry")).getByRole("switch");
    expect(se.getAttribute("aria-checked")).toBe("true");
    expect(se.hasAttribute("disabled")).toBe(true);
    fireEvent.click(ghSwitch);
    await waitFor(() => expect(ghSwitch.getAttribute("aria-checked")).toBe("true"));
    expect(calls.find((c) => c.cmd === "grants_set")?.args).toEqual({
      subject: { type: "thread", threadId: "th_1" },
      connectionId: "pc_gh",
      granted: true,
    });
  });
});

describe("F-PL-03 a build without plugins (the browser demo)", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend({
      plugins_catalog: () => {
        throw { code: "unavailable", message: "Plugins sign in to your own accounts, so they don't connect inside this demo." };
      },
      held_drafts_list: () => [],
    });
  });

  it("F-PL-03 says why there are no plugins, as information with nothing to retry", async () => {
    ui(<PluginsPage />);
    const note = await screen.findByTestId("plugins-unavailable");
    expect(note.textContent).toContain("your own accounts");
    expect(within(note).queryByRole("button")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
