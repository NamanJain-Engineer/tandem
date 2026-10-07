import { formatDate } from "./api";

/** The one-line plan summary on /billing (F-BL-02). */
export function planLine(me: {
  plan: string;
  status: string;
  paid_until: string | null;
  cancel_at_period_end: boolean;
}): string {
  if (me.plan === "free") return me.status === "past_due" ? "Free (payment failed)" : "Free";
  const name = me.plan === "team" ? "Team" : "Pro";
  if (me.cancel_at_period_end || me.status === "canceled")
    return `${name} until ${formatDate(me.paid_until)}`;
  if (me.status === "trialing") return `${name} trial`;
  if (me.status === "past_due") return `${name} · Payment failed — update your card`;
  return `${name} · Active`;
}
