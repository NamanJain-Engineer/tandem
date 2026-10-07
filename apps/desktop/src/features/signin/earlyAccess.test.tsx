import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { AuthState } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { UpgradeDialog } from "../billing/UpgradeDialog";
import { AccountPage } from "../settings/AccountPage";
import { SignIn } from "./SignIn";

const EARLY_ACCESS_NEW: AuthState = {
  signedIn: false,
  email: null,
  offline: false,
  offlineDaysLeft: null,
  hosted: false,
  pending: null,
  error: null,
};

const EARLY_ACCESS_IN_USE: AuthState = { ...EARLY_ACCESS_NEW, offline: true };

describe("early-access builds (D-105)", () => {
  beforeEach(() => resetStores());

  it("welcome offers the local Free plan and no sign-in", async () => {
    const calls = fakeBackend();
    useSession.setState({ auth: EARLY_ACCESS_NEW });
    render(
      <UiProvider>
        <SignIn />
      </UiProvider>,
    );
    expect(screen.getByText("Free plan")).toBeTruthy();
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

  it("Account & Plan explains early access without a sign-in button", () => {
    fakeBackend();
    useSession.setState({ auth: EARLY_ACCESS_IN_USE });
    render(
      <UiProvider>
        <AccountPage />
      </UiProvider>,
    );
    expect(screen.getByTestId("account-early-access").textContent).toContain(
      "Pro and Team start with Vibezeo accounts",
    );
    expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();
  });

  it("the plans sheet can't start a checkout", () => {
    fakeBackend();
    useSession.setState({ auth: EARLY_ACCESS_IN_USE });
    useUi.setState({ upgrade: { open: true } });
    render(
      <UiProvider>
        <UpgradeDialog />
      </UiProvider>,
    );
    expect(screen.getByTestId("upgrade-early-access")).toBeTruthy();
    expect((screen.getByTestId("upgrade-pro") as HTMLButtonElement).disabled).toBe(true);
  });
});
