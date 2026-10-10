import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { BOOT, FREE_VIEW, SIGNED_IN, fakeBackend, resetStores } from "../../test/fakeBackend";
import { StatusBar } from "./StatusBar";
import { useBoot } from "./useBoot";

const show = () =>
  render(
    <UiProvider>
      <StatusBar />
    </UiProvider>,
  );

describe("status bar connection (UI §3.6)", () => {
  beforeEach(() => {
    resetStores();
    fakeBackend();
    useBoot.setState({ boot: { ...BOOT, version: "0.4.0" } });
  });

  it("an early-access build (no hosted services) claims no connection, only its version", () => {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: false },
      entitlement: { ...FREE_VIEW, online: true },
    });
    show();
    expect(screen.queryByTestId("connection-state")).toBeNull();
    expect(screen.getByText("v0.4.0")).toBeTruthy();
  });

  it("a hosted build says Online, and Offline after a failed refresh", () => {
    useSession.setState({ auth: SIGNED_IN, entitlement: { ...FREE_VIEW, online: true } });
    const { unmount } = show();
    expect(screen.getByTestId("connection-state").textContent).toBe("Online");
    unmount();
    useSession.setState({ auth: SIGNED_IN, entitlement: { ...FREE_VIEW, online: false } });
    show();
    expect(screen.getByTestId("connection-state").textContent).toBe("Offline · local features still work");
  });
});
