import { describe, expect, it } from "vitest";
import { effectivePlan, type SubscriptionRow } from "./plan";

const now = new Date("2026-10-01T12:00:00Z");
const day = 86_400_000;
const base: SubscriptionRow = {
  plan: "pro",
  status: "active",
  currentPeriodEnd: new Date(now.getTime() + 10 * day),
  cancelAtPeriodEnd: false,
  pastDueSince: null,
  trialEnd: null,
};

describe("effectivePlan (F-BL-02, F-BL-04)", () => {
  it("no subscription is Free", () => {
    expect(effectivePlan(null, now)).toMatchObject({ plan: "free", status: "none" });
  });

  it("active and trialing are paid", () => {
    expect(effectivePlan(base, now).plan).toBe("pro");
    expect(effectivePlan({ ...base, status: "trialing" }, now).plan).toBe("pro");
  });

  it("F-BL-02.1 cancel at period end is Pro until the date, then Free", () => {
    const cancelled = { ...base, cancelAtPeriodEnd: true };
    expect(effectivePlan(cancelled, now)).toMatchObject({
      plan: "pro",
      paidUntil: cancelled.currentPeriodEnd,
    });
    expect(effectivePlan(cancelled, new Date(now.getTime() + 11 * day)).plan).toBe("free");
  });

  it("F-BL-04.1 past_due keeps paid limits for 7 days, then Free", () => {
    const pastDue = { ...base, status: "past_due" as const, pastDueSince: now };
    expect(effectivePlan(pastDue, new Date(now.getTime() + 6 * day)).plan).toBe("pro");
    expect(effectivePlan(pastDue, new Date(now.getTime() + 8 * day))).toMatchObject({
      plan: "free",
      status: "past_due",
    });
  });

  it("canceled is paid only until period end", () => {
    const canceled = { ...base, status: "canceled" as const };
    expect(effectivePlan(canceled, now).plan).toBe("pro");
    expect(effectivePlan(canceled, new Date(now.getTime() + 11 * day)).plan).toBe("free");
  });
});
