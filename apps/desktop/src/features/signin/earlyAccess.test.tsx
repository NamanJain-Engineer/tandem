import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { AuthState } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { AccountPage } from "../settings/AccountPage";
import { SignIn } from "./SignIn";

const EARLY_ACCESS_NEW: AuthState = {
  signedIn: false,
  email: null,
  offline: false,
  hosted: false,
  pending: null,
  error: null,
};

const EARLY_ACCESS_IN_USE: AuthState = { ...EARLY_ACCESS_NEW, offline: true };

describe("early-access builds (D-105)", () => {
  beforeEach(() => resetStores());

  it("F-WB-01 welcome says it's free, with no account and nothing sent, and offers no sign-in (D-869)", async () => {
    const calls = fakeBackend();
    useSession.setState({ auth: EARLY_ACCESS_NEW });
    render(
      <UiProvider>
        <SignIn />
      </UiProvider>,
    );
    expect(screen.getByTestId("signin-free").textContent).toBe("Free · no account · nothing sent to Vibezeo");
    // No plan, limit or price on the welcome (D-2900).
    expect(screen.queryByText(/Free plan|panes|workspaces|\bPro\b|\bTeam\b|\$/)).toBeNull();
    // A released build doesn't read as unfinished.
    expect(screen.queryByText(/early access|at launch/i)).toBeNull();
    expect(screen.queryByText("Continue with Google")).toBeNull();
    expect(screen.queryByText("Continue with email")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Start using Vibezeo" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "auth_continue_offline")).toBe(true));
    expect(calls.some((c) => c.cmd === "auth_sign_in")).toBe(false);
  });

  it("hosted builds keep the sign-in buttons", () => {
    fakeBackend();
    useSession.setState({ auth: { ...EARLY_ACCESS_NEW, hosted: true } });
    render(
      <UiProvider>
        <SignIn />
      </UiProvider>,
    );
    expect(screen.getByText("Continue with Google")).toBeTruthy();
    expect(screen.getByText("Use Vibezeo offline")).toBeTruthy();
  });

  it("Account explains everything runs with no account, and offers Tell me when accounts open, not sign-in", () => {
    fakeBackend();
    useSession.setState({ auth: EARLY_ACCESS_IN_USE });
    render(
      <UiProvider>
        <AccountPage />
      </UiProvider>,
    );
    const text = screen.getByTestId("account-early-access").textContent ?? "";
    expect(text).toContain("Everything runs on this computer, with no account.");
    expect(text).toContain("come with Vibezeo accounts");
    expect(screen.getByRole("button", { name: "Tell me when accounts open" })).toBeTruthy();
    expect(text).not.toMatch(/\bPro\b|\bTeam\b|plan/);
    expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();
  });
});
