// Posts a signed Stripe fixture for a user: `scripts/stripe-fixture.sh <event> <email>`.
// Event ids are derived from the user, so replaying a fixture is idempotent.
import { readFileSync } from "node:fs";
import pg from "pg";
import { signPayload } from "../src/providers/billing/signature";

const [event, email, nonce] = process.argv.slice(2);
if (!event || !email) {
  console.error("usage: stripe-fixture <event> <email> [nonce]");
  process.exit(2);
}
const db = new pg.Client({
  connectionString: process.env.DATABASE_URL ?? "postgres://tandem:tandem@localhost:5432/tandem",
});
await db.connect();
const user = await db.query<{ id: string }>('SELECT id FROM "user" WHERE email = $1', [email]);
const userId = user.rows[0]?.id;
if (!userId) {
  console.error(`no user ${email}`);
  process.exit(1);
}
const customer =
  (
    await db.query<{ customer_id: string }>("SELECT customer_id FROM billing_customers WHERE owner_id = $1", [
      userId,
    ])
  ).rows[0]?.customer_id ?? `cus_fixture_${userId}`;
const subscription =
  (
    await db.query<{ stripe_subscription_id: string }>(
      "SELECT stripe_subscription_id FROM subscriptions WHERE owner_id = $1 ORDER BY updated_at DESC LIMIT 1",
      [userId],
    )
  ).rows[0]?.stripe_subscription_id ?? `sub_fixture_${userId}`;
await db.end();

const now = Math.floor(Date.now() / 1000);
const body = readFileSync(new URL(`../test/fixtures/stripe/${event}.json`, import.meta.url), "utf8")
  .replaceAll("{{USER_ID}}", userId)
  .replaceAll("{{CUSTOMER}}", customer)
  .replaceAll("{{SUBSCRIPTION}}", subscription)
  .replaceAll("{{NOW_PLUS_30D}}", String(now + 30 * 86_400))
  .replaceAll("{{NOW}}", String(now));
const parsed = JSON.parse(body) as { id: string };
// A nonce makes a new event id (E2E re-runs); without one, replays are no-ops.
const payload = JSON.stringify({ ...parsed, id: nonce ? `${parsed.id}_${nonce}` : parsed.id, created: now });
const api = process.env.TANDEM_API_URL ?? "http://127.0.0.1:8787";
const res = await fetch(`${api}/v1/webhooks/stripe`, {
  method: "POST",
  headers: {
    "content-type": "application/json",
    "stripe-signature": signPayload(payload, process.env.STRIPE_WEBHOOK_SECRET ?? "whsec_mock", now),
  },
  body: payload,
});
console.log(`${event} for ${email}: ${res.status}`);
if (!res.ok) process.exit(1);
