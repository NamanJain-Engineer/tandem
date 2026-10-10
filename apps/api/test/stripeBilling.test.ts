import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, it } from "vitest";
import { StripeBilling } from "../src/providers/billing/stripe";

let server: Server | null = null;
afterEach(async () => {
  await new Promise<void>((resolve) => (server ? server.close(() => resolve()) : resolve()));
  server = null;
});

/** A local stand-in for Stripe's API that answers every request with `status` and `body`. */
async function stripeAnswering(status: number, body: unknown) {
  const requests: string[] = [];
  server = createServer((req, res) => {
    requests.push(`${req.method} ${req.url}`);
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  });
  await new Promise<void>((resolve) => server?.listen(0, "127.0.0.1", resolve));
  const { port } = server.address() as AddressInfo;
  return { billing: new StripeBilling("sk_test_local", "whsec_local", `http://127.0.0.1:${port}`), requests };
}

describe("F-TL-05 Stripe customer deletion", () => {
  it("F-TL-05 deleting the customer is one call to Stripe", async () => {
    const stripe = await stripeAnswering(200, { id: "cus_1", object: "customer", deleted: true });
    await stripe.billing.deleteCustomer("cus_1");
    expect(stripe.requests).toEqual(["DELETE /v1/customers/cus_1"]);
  });

  it("F-TL-05 a customer that's already gone counts as deleted, so a retried purge goes through", async () => {
    const stripe = await stripeAnswering(404, {
      error: {
        type: "invalid_request_error",
        code: "resource_missing",
        param: "id",
        message: "No such customer: 'cus_gone'",
      },
    });
    await expect(stripe.billing.deleteCustomer("cus_gone")).resolves.toBeUndefined();
  });

  it("F-TL-05 any other refusal fails the purge, which is retried on the next pass", async () => {
    const stripe = await stripeAnswering(401, {
      error: { type: "invalid_request_error", message: "Invalid API Key provided." },
    });
    await expect(stripe.billing.deleteCustomer("cus_1")).rejects.toThrow(/Invalid API Key/);
  });
});
