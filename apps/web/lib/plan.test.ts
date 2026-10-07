import { describe, expect, it } from "vitest";
import { planLine } from "./plan";

describe("planLine", () => {
  it("F-BL-02 shows Pro until the period end after cancelling", () => {
    expect(
      planLine({
        plan: "pro",
        status: "active",
        paid_until: "2026-11-12T00:00:00Z",
        cancel_at_period_end: true,
      }),
    ).toMatch(/^Pro until /);
    expect(planLine({ plan: "pro", status: "trialing", paid_until: null, cancel_at_period_end: false })).toBe(
      "Pro trial",
    );
    expect(planLine({ plan: "free", status: "none", paid_until: null, cancel_at_period_end: false })).toBe(
      "Free",
    );
  });
});
