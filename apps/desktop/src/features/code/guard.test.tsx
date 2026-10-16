import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { GuardCard } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { followRunaway, useRunaway } from "../../stores/runaway";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { guardLimits } from "../settings/RunawayGuard";
import { GuardBanner, guardLine } from "./GuardCard";

const card = (patch: Partial<GuardCard> = {}): GuardCard => ({
  paneId: "pn_1",
  trip: { kind: "working", minutes: 90 },
  state: "tripped",
  title: "fix-auth has been working for 90 min",
  reason: null,
  ...patch,
});

const show = () =>
  render(
    <UiProvider>
      <GuardBanner paneId="pn_1" />
    </UiProvider>,
  );

beforeEach(() => resetStores());
afterEach(() => setInvokeBackend(null));

describe("runaway guard card (F-AC-09, UI-170)", () => {
  it("F-AC-09.1 a tripped limit shows its title with Pause and Dismiss", () => {
    useRunaway.getState().set("pn_1", card());
    show();
    const banner = screen.getByTestId("guard-banner");
    expect(banner.dataset.state).toBe("tripped");
    expect(banner.textContent).toContain("fix-auth has been working for 90 min");
    expect(screen.getByTestId("guard-pause")).toBeTruthy();
    expect(screen.getByTestId("guard-dismiss")).toBeTruthy();
    expect(screen.queryByTestId("guard-resume")).toBeNull();
  });

  it("F-AC-09.2 Pause asks the core; a paused card offers Resume", async () => {
    const calls = fakeBackend({
      runaway_pause: () => null,
      runaway_resume: () => null,
      runaway_list: () => [],
    });
    useRunaway.getState().set("pn_1", card());
    show();
    fireEvent.click(screen.getByTestId("guard-pause"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "runaway_pause")).toBe(true));
    useRunaway.getState().set("pn_1", card({ state: "paused" }));
    fireEvent.click(await screen.findByTestId("guard-resume"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "runaway_resume")).toBe(true));
    expect(guardLine(card({ state: "pausing" }))).toBe("Pausing…");
    expect(guardLine(card({ state: "cant_pause", reason: null }))).toBe(
      "Couldn't pause: send it a message or stop it.",
    );
  });

  it("F-AC-09.5 runaway:changed adds and removes the card", async () => {
    fakeBackend({ runaway_list: () => [] });
    const stop = followRunaway();
    emitLocal("runaway:changed", { paneId: "pn_1", card: card() });
    await waitFor(() => expect(useRunaway.getState().byPane.pn_1).toBeTruthy());
    emitLocal("runaway:changed", { paneId: "pn_1", card: null });
    await waitFor(() => expect(useRunaway.getState().byPane.pn_1).toBeUndefined());
    stop();
  });

  it("F-AC-09.6 limits turned off stay off; missing ones take the defaults", () => {
    expect(guardLimits({ subagents: null })).toEqual({
      workingMinutes: 90,
      windowPoints: 30,
      subagents: null,
      circles: true,
    });
    expect(guardLimits(null).workingMinutes).toBe(90);
  });
});
