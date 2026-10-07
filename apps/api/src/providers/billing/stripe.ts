import Stripe from "stripe";
import type { BillingEvent, BillingProvider, CheckoutRequest } from "./types";

/** Real Stripe. In dev, STRIPE_API_BASE points at stripe-mock (:12111). */
export class StripeBilling implements BillingProvider {
  readonly kind = "stripe" as const;
  private readonly stripe: Stripe;

  constructor(
    secretKey: string,
    private readonly webhookSecret: string,
    apiBase?: string,
  ) {
    const url = apiBase ? new URL(apiBase) : undefined;
    this.stripe = new Stripe(secretKey, {
      ...(url
        ? {
            host: url.hostname,
            port: Number(url.port || 80),
            protocol: url.protocol.replace(":", "") as "http" | "https",
          }
        : {}),
      maxNetworkRetries: 1,
      telemetry: false,
    });
  }

  async createCheckout(req: CheckoutRequest) {
    const session = await this.stripe.checkout.sessions.create({
      mode: req.mode,
      customer: req.customerId,
      customer_email: req.customerId ? undefined : req.email,
      line_items: [{ price: req.priceId, quantity: req.quantity }],
      allow_promotion_codes: req.promo ? undefined : true,
      discounts: req.promo ? [{ promotion_code: req.promo }] : undefined,
      success_url: req.successUrl,
      cancel_url: req.cancelUrl,
      metadata: req.metadata,
      ...(req.mode === "subscription"
        ? {
            payment_method_collection: "always" as const,
            subscription_data: { trial_period_days: req.trialDays, metadata: req.metadata },
          }
        : {}),
    });
    return { id: session.id, url: session.url ?? req.cancelUrl };
  }

  async createPortal(customerId: string, returnUrl: string) {
    const portal = await this.stripe.billingPortal.sessions.create({
      customer: customerId,
      return_url: returnUrl,
    });
    return { url: portal.url };
  }

  constructEvent(rawBody: string, signature: string | undefined): BillingEvent {
    if (!signature) throw new Error("missing signature");
    return this.stripe.webhooks.constructEvent(
      rawBody,
      signature,
      this.webhookSecret,
    ) as unknown as BillingEvent;
  }

  async refundLatestCharge(subscriptionId: string) {
    const invoices = await this.stripe.invoices.list({ subscription: subscriptionId, limit: 1 });
    const invoice = invoices.data[0] as unknown as
      { charge?: string | null; payment_intent?: string | null } | undefined;
    if (invoice?.payment_intent) await this.stripe.refunds.create({ payment_intent: invoice.payment_intent });
    else if (invoice?.charge) await this.stripe.refunds.create({ charge: invoice.charge });
  }

  async updateQuantity(subscriptionId: string, seats: number) {
    const sub = await this.stripe.subscriptions.retrieve(subscriptionId);
    const item = sub.items.data[0];
    if (item) await this.stripe.subscriptionItems.update(item.id, { quantity: seats });
  }

  async cancelNow(subscriptionId: string) {
    await this.stripe.subscriptions.cancel(subscriptionId);
  }
}
