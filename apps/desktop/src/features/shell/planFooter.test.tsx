import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { SIGNED_IN, resetStores } from "../../test/fakeBackend";
import { PlanFooter } from "./PlanFooter";

const footer = () => screen.getByTestId("plan-footer").textContent;

describe("rail plan footer (F-SH-03)", () => {
  beforeEach(() => resetStores());

  it("F-SH-03 an early-access build (no hosted services) runs the Free plan and says FREE, not OFFLINE", () => {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: false },
      entitlement: null,
    });
    render(
      <UiProvider>
        <PlanFooter />
      </UiProvider>,
    );
    expect(footer()).toBe("FREE");
  });

  it("F-SH-03 a hosted build used offline says OFFLINE", () => {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: true },
      entitlement: null,
    });
    render(
      <UiProvider>
        <PlanFooter />
      </UiProvider>,
    );
    expect(footer()).toBe("OFFLINE");
  });
});
