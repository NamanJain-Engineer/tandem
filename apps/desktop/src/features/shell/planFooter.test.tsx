import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { useSession } from "../../stores/session";
import { FREE_VIEW, SIGNED_IN, resetStores } from "../../test/fakeBackend";
import { PlanFooter } from "./PlanFooter";

const footer = () => screen.getByTestId("plan-footer").textContent;

describe("rail plan footer (F-SH-03)", () => {
  beforeEach(() => resetStores());

  it("F-SH-03 an early-access build (no hosted services) says Free, not Offline (D-869)", () => {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: false },
      entitlement: null,
    });
    render(
      <UiProvider>
        <PlanFooter />
      </UiProvider>,
    );
    expect(footer()).toBe("Free");
  });

  it("F-SH-03 a hosted build used offline says Offline", () => {
    useSession.setState({
      auth: { ...SIGNED_IN, signedIn: false, email: null, offline: true, hosted: true },
      entitlement: null,
    });
    render(
      <UiProvider>
        <PlanFooter />
      </UiProvider>,
    );
    expect(footer()).toBe("Offline");
  });

  it("F-SH-03 signed in, it shows the credits hosted features use and never a plan name (D-2904)", () => {
    useSession.setState({
      auth: SIGNED_IN,
      entitlement: {
        ...FREE_VIEW,
        plan: "pro",
        credits: { ...FREE_VIEW.credits!, monthly: 9_000, purchased: 1_000 },
      },
    });
    render(
      <UiProvider>
        <PlanFooter />
      </UiProvider>,
    );
    expect(footer()).toBe("10,000 credits");
  });
});
