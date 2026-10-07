// `pnpm db:seed`: the MILESTONES §1 accounts, all verified, password
// `Tandem-dev-pass-1`. Safe to run repeatedly.
import { eq } from "drizzle-orm";
import { fileURLToPath } from "node:url";
import { buildDeps } from "../buildDeps";
import { loadEnv } from "../env";
import { newId } from "../lib/ids";
import { MemoryEmail } from "../providers/email/smtp";
import { grantMonthly } from "../services/ledger";
import { billingCustomers, subscriptions, user } from "./schema";

export const SEED_PASSWORD = "Tandem-dev-pass-1";
export const SEED_USERS = ["free", "pro", "pastdue", "lapsed"] as const;

export async function seed(deps: ReturnType<typeof buildDeps>["deps"]): Promise<void> {
  const now = deps.clock.now();
  const day = 86_400_000;
  for (const name of SEED_USERS) {
    const email = `${name}@tandem.test`;
    let [u] = await deps.db.select().from(user).where(eq(user.email, email)).limit(1);
    if (!u) {
      await deps.auth.api.signUpEmail({
        body: { email, password: SEED_PASSWORD, name: name[0]?.toUpperCase() + name.slice(1) },
      });
      [u] = await deps.db.select().from(user).where(eq(user.email, email)).limit(1);
    }
    if (!u) throw new Error(`seed: couldn't create ${email}`);
    await deps.db.update(user).set({ emailVerified: true }).where(eq(user.id, u.id));
    await deps.db.delete(subscriptions).where(eq(subscriptions.ownerId, u.id));
    const base = {
      ownerKind: "user" as const,
      ownerId: u.id,
      plan: "pro" as const,
      interval: "month" as const,
      seats: 1,
      currentPeriodStart: new Date(now.getTime() - 5 * day),
      currentPeriodEnd: new Date(now.getTime() + 25 * day),
      cancelAtPeriodEnd: false,
      updatedAt: now,
    };
    await deps.db
      .insert(billingCustomers)
      .values({ ownerKind: "user", ownerId: u.id, customerId: `cus_seed_${name}` })
      .onConflictDoNothing();
    if (name === "pro") {
      await deps.db
        .insert(subscriptions)
        .values({
          ...base,
          id: newId("sub"),
          stripeSubscriptionId: `sub_seed_${name}`,
          status: "active",
          firstPaidAt: base.currentPeriodStart,
        });
      await deps.db.transaction((tx) =>
        grantMonthly(tx, { kind: "user", id: u.id }, 1, base.currentPeriodEnd, `seed_${name}`),
      );
    } else if (name === "pastdue") {
      await deps.db
        .insert(subscriptions)
        .values({
          ...base,
          id: newId("sub"),
          stripeSubscriptionId: `sub_seed_${name}`,
          status: "past_due",
          pastDueSince: now,
        });
    } else if (name === "lapsed") {
      await deps.db.insert(subscriptions).values({
        ...base,
        id: newId("sub"),
        stripeSubscriptionId: `sub_seed_${name}`,
        status: "canceled",
        currentPeriodStart: new Date(now.getTime() - 40 * day),
        currentPeriodEnd: new Date(now.getTime() - 10 * day),
      });
    }
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { deps, close } = buildDeps(loadEnv(), { email: new MemoryEmail() });
  seed(deps)
    .then(() =>
      console.log(
        `seeded ${SEED_USERS.map((n) => `${n}@tandem.test`).join(", ")} (password ${SEED_PASSWORD})`,
      ),
    )
    .catch((err: unknown) => {
      console.error(err);
      process.exitCode = 1;
    })
    .finally(() => void close());
}
