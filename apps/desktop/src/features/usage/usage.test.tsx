import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { UsageReport } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { shareCard } from "./shareCard";
import { UsageDialog } from "./UsagePage";

const days = Array.from({ length: 84 }, (_, i) => {
  const d = new Date(Date.UTC(2026, 6, 19 + i));
  const date = d.toISOString().slice(0, 10);
  return { date, tokens: i === 83 ? 12_000 : i === 82 ? 3_000 : 0, byEngine: {} };
});

const REPORT: UsageReport = {
  since: days[0]?.date ?? "",
  until: days[83]?.date ?? "",
  total: 15_000,
  days,
  engines: [
    { engineId: "claude", label: "Claude Code", tokens: 15_000, sessions: 3 },
    { engineId: "codex", label: "Codex", tokens: 0, sessions: 0 },
  ],
  accounts: [{ accountId: "acc_1", engineId: "claude", label: "Work", tokens: 15_000, sessions: 3 }],
  workspaces: [{ name: "Shop", path: "/home/ana/shop", tokens: 15_000 }],
  skippedLines: 2,
  filesRead: 4,
  unsupported: ["Gemini CLI"],
};

const page = () =>
  render(
    <UiProvider>
      <UsageDialog open onOpenChange={() => undefined} />
    </UiProvider>,
  );

beforeEach(() => resetStores());

describe("F-AC-07 usage page", () => {
  it("F-AC-07.1 shows the total, the 12-week heatmap, engines, accounts and top workspaces", async () => {
    fakeBackend({ usage_report: () => REPORT });
    page();
    await screen.findByTestId("usage-page");
    expect(screen.getByTestId("usage-total").textContent).toBe("15K tokens");
    expect(screen.getByRole("img", { name: "Tokens per day, last 12 weeks" }).querySelectorAll("[data-date]")).toHaveLength(84);
    expect(screen.getByTestId("usage-engines").textContent).toContain("Claude Code");
    expect(screen.getByTestId("usage-accounts").textContent).toContain("Work");
    expect(screen.getByTestId("usage-workspaces").textContent).toContain("Shop");
  });

  it("F-AC-07.2 says how many lines were skipped, and which engines aren't counted", async () => {
    fakeBackend({ usage_report: () => REPORT });
    page();
    expect((await screen.findByTestId("usage-skipped")).textContent).toContain("2 lines skipped");
    expect(screen.getByTestId("usage-unsupported").textContent).toContain("Gemini CLI");
  });

  it("empty, loading and error states", async () => {
    const pending: { resolve?: (r: UsageReport) => void } = {};
    fakeBackend({ usage_report: () => new Promise<UsageReport>((r) => (pending.resolve = r)) });
    const { unmount } = page();
    expect(screen.getByTestId("usage-loading")).toBeTruthy();
    await waitFor(() => expect(pending.resolve).toBeDefined());
    pending.resolve?.({ ...REPORT, total: 0, engines: [], accounts: [], workspaces: [], days: days.map((d) => ({ ...d, tokens: 0 })) });
    expect((await screen.findByTestId("usage-empty")).textContent).toContain("No usage found in the last 12 weeks.");
    unmount();
    fakeBackend({
      usage_report: () => {
        throw new Error("boom");
      },
    });
    page();
    await waitFor(() => expect(screen.getByTestId("usage-error")).toBeTruthy());
  });

  it("the share card shows engines with usage only, and workspace names only when asked", () => {
    const card = shareCard(REPORT, false);
    expect(card.engines).toEqual([{ label: "Claude Code", tokens: "15K" }]);
    expect(card.workspaces).toEqual([]);
    expect(shareCard(REPORT, true).workspaces).toEqual([{ name: "Shop", tokens: "15K" }]);
  });
});
