import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AuthState, EntitlementView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import en from "../../i18n/en.json";
import { setInvokeBackend } from "../../ipc/invoke";
import { startCall, useConductor } from "../../stores/conductor";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { FREE_VIEW, fakeBackend, resetStores } from "../../test/fakeBackend";
import { SettingsView } from "../settings/SettingsView";
import { SignIn } from "../signin/SignIn";
import { hostedTooltip } from "./ComesWithAccounts";

// The download (TANDEM_HOSTED=0): no account, nothing hosted. DESIGN §5.5 and
// §9.5 (D-869, D-2903): it never names a plan, a price, an upgrade or credits.
const NO_ACCOUNTS: AuthState = {
  signedIn: false,
  email: null,
  offline: true,
  offlineDaysLeft: null,
  hosted: false,
  pending: null,
  error: null,
};
const DOWNLOAD: EntitlementView = { ...FREE_VIEW, source: "free", credits: null };
const BANNED = /credit|\bPro\b|premium|\bpaid\b|upgrade|trial|\$\d|free for now/i;

beforeEach(() => {
  resetStores();
  fakeBackend({
    entitlement_get: () => DOWNLOAD,
    auth_state: () => NO_ACCOUNTS,
    ssh_hosts_list: () => [],
    held_drafts_list: () => [],
  });
  useSession.setState({ auth: NO_ACCOUNTS, entitlement: DOWNLOAD });
});
afterEach(() => setInvokeBackend(null));

describe("the download build says nothing about plans or credits (D-2903, DESIGN §9.5)", () => {
  it("F-WB-01 every Settings page, the welcome screen and Settings' one free line", async () => {
    const { unmount } = render(
      <UiProvider>
        <SignIn />
      </UiProvider>,
    );
    expect(document.body.textContent).not.toMatch(BANNED);
    unmount();
    useSettings.setState({ values: { settings_last_page: "general" }, loaded: true });
    render(
      <UiProvider>
        <SettingsView />
      </UiProvider>,
    );
    const nav = screen.getByRole("navigation", { name: "Settings pages" });
    const pages = Array.from(nav.querySelectorAll("button")).map((b) => b.textContent ?? "");
    expect(pages.length).toBeGreaterThan(5);
    for (const name of pages) {
      await act(async () => fireEvent.click(screen.getByRole("button", { name })));
      expect(document.body.textContent, name).not.toMatch(BANNED);
    }
    expect(screen.queryByTestId("settings-free-line")?.textContent ?? en["settings.freeLine"]).toBe(
      "Vibezeo is free. If it helps you, tell a friend.",
    );
  });

  it("F-CN-01 Conductor's call command says it comes with accounts, with no plan or credits", async () => {
    await act(() => startCall());
    const message = useConductor.getState().view.error?.message ?? "";
    expect(message).toBe(
      "Conductor comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
    );
    expect(message).not.toMatch(BANNED);
  });

  it("F-DI-05 the hosted tooltips and labels use DESIGN §9.5's words, word for word", () => {
    expect(hostedTooltip()).toBe(
      "Comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
    );
    for (const key of [
      "hosted.withAccounts",
      "hosted.why",
      "hosted.notify",
      "dictate.engine.needsPro",
      "dictate.enhance.needsPro",
      "remote.earlyAccess",
      "settings.account.earlyAccess",
      "settings.dictionarySync.desc",
      "menu.earlyAccess",
      "signin.earlyAccess.note",
    ] as const)
      expect(en[key], key).not.toMatch(BANNED);
  });
});
