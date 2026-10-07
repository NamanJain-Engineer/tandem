// Stripe webhook handling (ARCHITECTURE §14.7). Each event is applied once:
// its id is inserted in the same transaction as its effects.
import { and, eq } from "drizzle-orm";
import { CREDIT_PACKS, type CreditPack } from "@tandem/protocol/plans";
import type { Deps } from "../deps";
import { billingCustomers, subscriptions, user, webhookEvents } from "../db/schema";
import { newId } from "../lib/ids";
import { logger } from "../lib/logger";
import type { BillingEvent } from "../providers/billing/types";
import { addPurchased, grantMonthly, type Owner } from "./ledger";

type Obj = Record<string, unknown>;
const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);
const num = (v: unknown): number | undefined => (typeof v === "number" ? v : undefined);
const obj = (v: unknown): Obj => (v && typeof v === "object" ? (v as Obj) : {});
const date = (sec: number | undefined): Date | null => (sec === undefined ? null : new Date(sec * 1000));

export type WebhookOutcome = { applied: boolean; notify: string[] };

type Tx = Parameters<Parameters<Deps["db"]["transaction"]>[0]>[0];

async function ownerFor(tx: Tx, object: Obj): Promise<Owner | null> {
  const meta = obj(object.metadata);
  const kind = str(meta.owner_kind);
  const id = str(meta.owner_id);
  if ((kind === "user" || kind === "org") && id) return { kind, id };
  const customer = str(object.customer);
  if (!customer) return null;
  const [row] = await tx
    .select()
    .from(billingCustomers)
    .where(eq(billingCustomers.customerId, customer))
    .limit(1);
  return row ? { kind: row.ownerKind, id: row.ownerId } : null;
}

function subscriptionFields(object: Obj) {
  const item = obj((obj(object.items).data as unknown[] | undefined)?.[0]);
  const meta = obj(object.metadata);
  const plan = str(meta.plan) === "team" ? "team" : "pro";
  const interval = str(meta.interval) === "year" ? "year" : "month";
  const start = num(object.current_period_start) ?? num(item.current_period_start);
  const end = num(object.current_period_end) ?? num(item.current_period_end);
  return {
    plan: plan as "pro" | "team",
    interval: interval as "month" | "year",
    seats: num(item.quantity) ?? num(object.quantity) ?? 1,
    currentPeriodStart: date(start) ?? new Date(),
    currentPeriodEnd: date(end) ?? new Date(),
    cancelAtPeriodEnd: object.cancel_at_period_end === true,
    trialEnd: date(num(object.trial_end)),
  };
}

const STATUSES = ["trialing", "active", "past_due", "canceled", "unpaid", "incomplete"] as const;
type Status = (typeof STATUSES)[number];

async function upsertSubscription(deps: Deps, tx: Tx, object: Obj, deleted: boolean): Promise<Owner | null> {
  const owner = await ownerFor(tx, object);
  const stripeId = str(object.id);
  if (!owner || !stripeId) return null;
  const raw = str(object.status) ?? "incomplete";
  const status: Status = deleted
    ? "canceled"
    : (STATUSES as readonly string[]).includes(raw)
      ? (raw as Status)
      : "incomplete";
  const fields = subscriptionFields(object);
  const now = deps.clock.now();
  const [existing] = await tx
    .select()
    .from(subscriptions)
    .where(eq(subscriptions.stripeSubscriptionId, stripeId))
    .limit(1);
  const pastDueSince = status === "past_due" ? (existing?.pastDueSince ?? now) : null;
  if (existing) {
    await tx
      .update(subscriptions)
      .set({ ...fields, status, pastDueSince, updatedAt: now })
      .where(eq(subscriptions.id, existing.id));
  } else {
    await tx.insert(subscriptions).values({
      id: newId("sub"),
      stripeSubscriptionId: stripeId,
      ownerKind: owner.kind,
      ownerId: owner.id,
      ...fields,
      status,
      pastDueSince,
      updatedAt: now,
    });
  }
  const customer = str(object.customer);
  if (customer) {
    await tx
      .insert(billingCustomers)
      .values({ ownerKind: owner.kind, ownerId: owner.id, customerId: customer })
      .onConflictDoNothing();
  }
  return owner;
}

async function emailFor(tx: Tx, owner: Owner): Promise<string | null> {
  if (owner.kind !== "user") return null;
  const [u] = await tx.select({ email: user.email }).from(user).where(eq(user.id, owner.id)).limit(1);
  return u?.email ?? null;
}

export async function handleEvent(deps: Deps, event: BillingEvent): Promise<WebhookOutcome> {
  const mails: Array<{ to: string; subject: string; text: string }> = [];
  const outcome = await deps.db.transaction(async (tx) => {
    const inserted = await tx
      .insert(webhookEvents)
      .values({ id: event.id, type: event.type })
      .onConflictDoNothing()
      .returning({ id: webhookEvents.id });
    if (inserted.length === 0) return { applied: false, notify: [] as string[] };

    const object = obj(event.data.object);
    const notify = new Set<string>();
    switch (event.type) {
      case "checkout.session.completed": {
        const owner = await ownerFor(tx, object);
        if (!owner) break;
        const customer = str(object.customer);
        if (customer) {
          await tx
            .insert(billingCustomers)
            .values({ ownerKind: owner.kind, ownerId: owner.id, customerId: customer })
            .onConflictDoNothing();
        }
        const pack = str(obj(object.metadata).pack) as CreditPack | undefined;
        if (str(object.mode) === "payment" && pack && pack in CREDIT_PACKS) {
          await addPurchased(tx, owner, CREDIT_PACKS[pack].credits, event.id);
        }
        if (owner.kind === "user") notify.add(owner.id);
        break;
      }
      case "customer.subscription.created":
      case "customer.subscription.updated":
      case "customer.subscription.deleted": {
        const owner = await upsertSubscription(deps, tx, object, event.type.endsWith("deleted"));
        if (owner?.kind === "user") notify.add(owner.id);
        break;
      }
      case "invoice.paid": {
        const subId =
          str(object.subscription) ?? str(obj(obj(object.parent).subscription_details).subscription);
        if (!subId) break;
        const [sub] = await tx
          .select()
          .from(subscriptions)
          .where(eq(subscriptions.stripeSubscriptionId, subId))
          .limit(1);
        const owner = sub ? { kind: sub.ownerKind, id: sub.ownerId } : await ownerFor(tx, object);
        if (!owner) break;
        const line = obj(((obj(object.lines).data as unknown[] | undefined) ?? [])[0]);
        const periodEnd = date(num(obj(line.period).end)) ?? sub?.currentPeriodEnd ?? deps.clock.now();
        await grantMonthly(tx, owner, sub?.seats ?? 1, periodEnd, event.id);
        if (sub && !sub.firstPaidAt && (num(object.amount_paid) ?? 0) > 0) {
          await tx
            .update(subscriptions)
            .set({ firstPaidAt: deps.clock.now() })
            .where(eq(subscriptions.id, sub.id));
        }
        if (owner.kind === "user") notify.add(owner.id);
        break;
      }
      case "invoice.payment_failed": {
        const subId =
          str(object.subscription) ?? str(obj(obj(object.parent).subscription_details).subscription);
        const owner = await ownerFor(tx, object);
        if (subId) {
          await tx
            .update(subscriptions)
            .set({ status: "past_due", updatedAt: deps.clock.now() })
            .where(eq(subscriptions.stripeSubscriptionId, subId));
          const [sub] = await tx
            .select()
            .from(subscriptions)
            .where(eq(subscriptions.stripeSubscriptionId, subId))
            .limit(1);
          if (sub && !sub.pastDueSince) {
            await tx
              .update(subscriptions)
              .set({ pastDueSince: deps.clock.now() })
              .where(eq(subscriptions.id, sub.id));
          }
        }
        if (owner) {
          const to = await emailFor(tx, owner);
          if (to) {
            mails.push({
              to,
              subject: "Payment failed — update your card",
              text: `We couldn't charge your card for Tandem Pro. Update your payment method to keep Pro: ${deps.env.WEB_PUBLIC_URL}/billing\n\nIf it isn't updated within 7 days, your account moves to the Free plan. Your data stays as it is.`,
            });
          }
          if (owner.kind === "user") notify.add(owner.id);
        }
        break;
      }
      case "customer.subscription.trial_will_end": {
        const owner = await ownerFor(tx, object);
        const to = owner ? await emailFor(tx, owner) : null;
        if (to) {
          mails.push({
            to,
            subject: "Your Tandem trial ends soon",
            text: `Your Pro trial ends in 3 days. Manage billing: ${deps.env.WEB_PUBLIC_URL}/billing`,
          });
        }
        break;
      }
      default:
        logger.debug({ type: event.type }, "webhook type ignored");
    }
    await tx
      .update(webhookEvents)
      .set({ processedAt: deps.clock.now() })
      .where(eq(webhookEvents.id, event.id));
    return { applied: true, notify: [...notify] };
  });
  for (const mail of mails) {
    await deps.email.send(mail).catch((err: unknown) => logger.error({ err: String(err) }, "email failed"));
  }
  for (const userId of outcome.notify) {
    await deps.events.publish(userId, "entitlement");
  }
  return outcome;
}

export async function customerFor(deps: Deps, owner: Owner): Promise<string | null> {
  const [row] = await deps.db
    .select()
    .from(billingCustomers)
    .where(and(eq(billingCustomers.ownerKind, owner.kind), eq(billingCustomers.ownerId, owner.id)))
    .limit(1);
  return row?.customerId ?? null;
}
