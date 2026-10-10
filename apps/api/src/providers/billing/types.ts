export type CheckoutMode = "subscription" | "payment";

export type CheckoutRequest = {
  ownerKind: "user" | "org";
  ownerId: string;
  email: string;
  customerId?: string;
  priceId: string;
  quantity: number;
  trialDays?: number;
  promo?: string;
  successUrl: string;
  cancelUrl: string;
  mode: CheckoutMode;
  metadata: Record<string, string>;
};

/** The subset of a Stripe event Tandem reads. */
export type BillingEvent = {
  id: string;
  type: string;
  created: number;
  data: { object: Record<string, unknown> };
};

export interface BillingProvider {
  readonly kind: "stripe" | "mock";
  createCheckout(req: CheckoutRequest): Promise<{ id: string; url: string }>;
  createPortal(customerId: string, returnUrl: string): Promise<{ url: string }>;
  /** Verifies the `stripe-signature` header; throws on mismatch. */
  constructEvent(rawBody: string, signature: string | undefined): BillingEvent;
  refundLatestCharge(subscriptionId: string): Promise<void>;
  /** Refunds a one-time Checkout payment (a credit pack). */
  refundPayment(checkoutSessionId: string): Promise<void>;
  updateQuantity(subscriptionId: string, seats: number): Promise<void>;
  cancelNow(subscriptionId: string): Promise<void>;
}
