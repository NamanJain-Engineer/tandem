import { LIMITS, PAST_DUE_GRACE_DAYS, type Limits, type Plan, type PlanStatus } from "@tandem/protocol/plans";

export type SubscriptionRow = {
  plan: "pro" | "team";
  status: "trialing" | "active" | "past_due" | "canceled" | "unpaid" | "incomplete";
  currentPeriodEnd: Date;
  cancelAtPeriodEnd: boolean;
  pastDueSince: Date | null;
  trialEnd: Date | null;
  refundedAt?: Date | null;
};

export type EffectivePlan = { plan: Plan; status: PlanStatus; limits: Limits; paidUntil: Date | null };

const DAY_MS = 86_400_000;

/**
 * Lapse rules (PRODUCT_SPEC §3.2, F-BL-04): trialing or active is paid, except
 * a cancel-at-period-end subscription past its period end; past_due stays paid
 * for 7 days; a canceled subscription stays paid until its period end.
 * Everything else, and a refunded subscription (F-BL-05), is Free. Data is never touched.
 */
export function effectivePlan(sub: SubscriptionRow | null, now: Date): EffectivePlan {
  const free: EffectivePlan = {
    plan: "free",
    status: sub?.status ?? "none",
    limits: LIMITS.free,
    paidUntil: null,
  };
  if (!sub || sub.refundedAt) return free;
  const t = now.getTime();
  const end = sub.currentPeriodEnd.getTime();
  let paid = false;
  switch (sub.status) {
    case "trialing":
    case "active":
      paid = !(sub.cancelAtPeriodEnd && t >= end);
      break;
    case "past_due":
      paid = sub.pastDueSince !== null && t < sub.pastDueSince.getTime() + PAST_DUE_GRACE_DAYS * DAY_MS;
      break;
    case "canceled":
      paid = t < end;
      break;
    default:
      paid = false;
  }
  if (!paid) return free;
  return {
    plan: sub.plan,
    status: sub.status,
    limits: LIMITS[sub.plan],
    paidUntil: sub.cancelAtPeriodEnd || sub.status === "canceled" ? sub.currentPeriodEnd : null,
  };
}
