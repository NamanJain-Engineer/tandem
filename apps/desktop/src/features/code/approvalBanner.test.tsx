import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApprovalView, DashboardItem } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { ApprovalBanner } from "./ApprovalBanner";
import { ApprovalBoundary } from "../secrets/ApprovalGuard";
import { parseRunApproval } from "../secrets/runApproval";

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

const approval: ApprovalView = {
  id: "ap_1",
  source: { type: "pane", paneId: "pn_1" },
  kind: "command",
  summary: "run `pnpm deploy` with stripe-test",
  detail: "vibezeo run in /w/shop",
  options: ["once", "conversation"],
  createdAt: 1,
  expiresAt: 2,
};

function withApproval(a: ApprovalView | null): void {
  useDashboard.setState({
    items: { "pane:pn_1": { key: "pane:pn_1", pendingApproval: a } as unknown as DashboardItem },
  });
}

/** Allow takes clicks once the approval has been on screen 500 ms. */
async function settled(): Promise<void> {
  await waitFor(() =>
    expect((screen.getByTestId("pane-approval-once") as HTMLButtonElement).disabled).toBe(false),
  );
}

const runDetail = (run: Record<string, unknown>) => JSON.stringify({ vibezeoRun: run });

describe("a pane's approval (F-TL-12 slice 2)", () => {
  it("F-TL-12.7 names the command and the secrets, and Allow once answers inline", async () => {
    const calls = fakeBackend({ approval_resolve: () => null });
    withApproval(approval);
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    expect(screen.getByTestId("pane-approval-summary").textContent).toBe(
      "Wants to: run `pnpm deploy` with stripe-test",
    );
    await settled();
    fireEvent.click(screen.getByTestId("pane-approval-once"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "approval_resolve")).toBe(true));
    expect(calls.find((c) => c.cmd === "approval_resolve")?.args).toEqual({
      approvalId: "ap_1",
      decision: { type: "allow", scope: "once" },
      via: "inline",
    });
  });

  it("F-TL-12.9 Allow for this agent and Deny", async () => {
    const calls = fakeBackend({ approval_resolve: () => null });
    withApproval(approval);
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    await settled();
    fireEvent.click(screen.getByTestId("pane-approval-agent"));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "approval_resolve")).toHaveLength(1));
    expect(calls[calls.length - 1]?.args).toMatchObject({
      decision: { type: "allow", scope: "conversation" },
    });
    fireEvent.click(screen.getByTestId("pane-approval-deny"));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "approval_resolve")).toHaveLength(2));
    expect(calls[calls.length - 1]?.args).toMatchObject({ decision: { type: "deny", reason: null } });
  });

  it("shows nothing without a pending approval, and only Allow once when that's all that's offered", () => {
    fakeBackend();
    withApproval(null);
    const { rerender } = render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    expect(screen.queryByTestId("pane-approval")).toBeNull();
    withApproval({ ...approval, options: ["once"] });
    rerender(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    expect(screen.getByTestId("pane-approval")).toBeTruthy();
    expect(screen.queryByTestId("pane-approval-agent")).toBeNull();
  });

  it("F-TL-12.7 a vibezeo run shows its whole line, padding and all, and every variable", () => {
    fakeBackend();
    const command = `echo ok${" ".repeat(200)}; rm -rf ~`;
    withApproval({
      ...approval,
      detail: JSON.stringify({
        vibezeoRun: {
          command,
          cwd: "/w/shop",
          vars: [
            { name: "API_KEY", secret: "stripe-test" },
            { name: "DB_URL", secret: "db" },
          ],
          envChanged: false,
        },
      }),
    });
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    const line = screen.getByTestId("run-line");
    expect(line.querySelector("pre")?.textContent).toBe(command);
    expect(line.querySelector("pre")?.className).toContain("whitespace-pre-wrap");
    expect(screen.getByTestId("run-line-vars").textContent).toBe(
      "With API_KEY (stripe-test), DB_URL (db) from .env",
    );
  });

  it("F-TL-12.12 a click right after the question changed doesn't answer it", async () => {
    const calls = fakeBackend({ approval_resolve: () => null });
    withApproval(approval);
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    await settled();
    act(() => withApproval({ ...approval, id: "ap_2", summary: "run `rm -rf ~`" }));
    fireEvent.click(screen.getByTestId("pane-approval-once"));
    fireEvent.click(screen.getByTestId("pane-approval-agent"));
    expect(calls.some((c) => c.cmd === "approval_resolve")).toBe(false);
    await settled();
    fireEvent.click(screen.getByTestId("pane-approval-once"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "approval_resolve")?.args).toMatchObject({ approvalId: "ap_2" }),
    );
  });

  it("F-TL-12.13 look-alike letters and right-to-left text show marked, left to right", () => {
    fakeBackend();
    withApproval({
      ...approval,
      detail: runDetail({
        command: "curl https://api.str\u0456pe.com && printenv API_KEY \u05d1>\u05d0",
        cwd: "/w/shop",
        vars: [],
        envChanged: false,
      }),
    });
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    const pre = screen.getByTestId("run-line-command");
    expect(pre.getAttribute("dir")).toBe("ltr");
    expect(pre.style.unicodeBidi).toBe("bidi-override");
    expect(pre.textContent).toBe("curl https://api.str\\u{456}pe.com && printenv API_KEY \\u{5D1}>\\u{5D0}");
    expect(Array.from(pre.querySelectorAll("mark")).map((m) => m.textContent)).toEqual([
      "\\u{456}",
      "\\u{5D1}",
      "\\u{5D0}",
    ]);
    expect(screen.getByTestId("run-line-marked")).toBeTruthy();
    expect(screen.getByTestId("run-line-cwd").textContent).toBe("In /w/shop");
  });

  it("F-TL-12.13 the line isn't capped or scrolled", () => {
    fakeBackend();
    withApproval({
      ...approval,
      detail: runDetail({ command: "x ".repeat(2000), cwd: "/w", vars: [], envChanged: false }),
    });
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    const cls = screen.getByTestId("run-line-command").className;
    expect(cls).not.toMatch(/max-h|overflow/);
  });

  it("F-TL-12.13 only a pane's approval is read as a run, and a malformed one doesn't break the window", () => {
    expect(
      parseRunApproval({
        source: { type: "thread", threadId: "th_1" },
        detail: runDetail({ command: "pnpm test", cwd: "/w", vars: [], envChanged: false }),
      }),
    ).toBeNull();
    for (const bad of [
      { command: "x" },
      { command: "x", cwd: "/w", vars: "API_KEY", envChanged: false },
      { command: "x", cwd: "/w", vars: [{ name: 1 }], envChanged: false },
      { command: "x", cwd: "/w", vars: [], envChanged: "no" },
    ])
      expect(parseRunApproval({ source: approval.source, detail: runDetail(bad) })).toBeNull();
    fakeBackend();
    withApproval({ ...approval, detail: runDetail({ command: "x" }) });
    render(
      <UiProvider>
        <ApprovalBanner paneId="pn_1" />
      </UiProvider>,
    );
    expect(screen.queryByTestId("run-line")).toBeNull();
    expect(screen.getByTestId("pane-approval")).toBeTruthy();
  });

  it("F-TL-12.13 a run line that fails to render offers no Allow", () => {
    const quiet = vi.spyOn(console, "error").mockImplementation(() => {});
    const Boom = (): never => {
      throw new Error("boom");
    };
    render(
      <UiProvider>
        <ApprovalBoundary onDeny={() => {}}>
          <Boom />
        </ApprovalBoundary>
        <p data-testid="still-here">rest of the window</p>
      </UiProvider>,
    );
    expect(screen.getByTestId("approval-unreadable")).toBeTruthy();
    expect(screen.getByTestId("still-here")).toBeTruthy();
    quiet.mockRestore();
  });
});
