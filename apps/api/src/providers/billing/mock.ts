import { newId } from "../../lib/ids";
import type { Clock } from "../../lib/clock";
import type { BillingEvent, BillingProvider, CheckoutRequest } from "./types";
import { verifySignature } from "./signature";

export type PendingCheckout = CheckoutRequest & { id: string };

/**
 * BILLING_PROVIDER=mock: checkout returns the web page `/billing/mock`, which
 * calls `/v1/billing/mock/complete`; that emits synthetic Stripe events through
 * the same webhook handler. Webhook signatures are still verified.
 */
export class MockBilling implements BillingProvider {
  readonly kind = "mock" as const;
  readonly sessions = new Map<string, PendingCheckout>();
  readonly calls: string[] = [];

  constructor(
    private readonly webUrl: string,
    private readonly webhookSecret: string,
    private readonly clock: Clock,
  ) {}

  async createCheckout(req: CheckoutRequest) {
    const id = newId("cs_mock");
    this.sessions.set(id, { ...req, id });
    // `next`: where Stripe would return after paying (a pack goes back to /credits).
    const next = req.successUrl.replace("{CHECKOUT_SESSION_ID}", id);
    return {
      id,
      url: `${this.webUrl}/billing/mock?session=${encodeURIComponent(id)}&next=${encodeURIComponent(next)}`,
    };
  }

  async createPortal(customerId: string, returnUrl: string) {
    return {
      url: `${this.webUrl}/billing/mock-portal?customer=${encodeURIComponent(customerId)}&return=${encodeURIComponent(returnUrl)}`,
    };
  }

  constructEvent(rawBody: string, signature: string | undefined): BillingEvent {
    if (!verifySignature(rawBody, signature, this.webhookSecret, this.clock.nowSec())) {
      throw new Error("invalid signature");
    }
    return JSON.parse(rawBody) as BillingEvent;
  }

  async refundLatestCharge(subscriptionId: string) {
    this.calls.push(`refund:${subscriptionId}`);
  }

  async refundPayment(checkoutSessionId: string) {
    this.calls.push(`refund_payment:${checkoutSessionId}`);
  }

  async updateQuantity(subscriptionId: string, seats: number) {
    this.calls.push(`quantity:${subscriptionId}:${seats}`);
  }

  async cancelNow(subscriptionId: string) {
    this.calls.push(`cancel:${subscriptionId}`);
  }

  async deleteCustomer(customerId: string) {
    this.calls.push(`delete_customer:${customerId}`);
  }
}
