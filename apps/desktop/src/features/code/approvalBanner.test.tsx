import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ApprovalView, DashboardItem } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { ApprovalBanner } from "./ApprovalBanner";

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
});
