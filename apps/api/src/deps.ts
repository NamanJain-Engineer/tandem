import type { Redis } from "ioredis";
import type { Auth } from "./auth/better-auth";
import type { KeySet } from "./auth/keys";
import type { Db } from "./db/client";
import type { Env } from "./env";
import type { Clock } from "./lib/clock";
import type { AiProvider } from "./providers/ai/types";
import type { BillingProvider } from "./providers/billing/types";
import type { RealtimeProvider } from "./providers/realtime/types";
import type { SttProvider } from "./providers/stt/types";
import type { BreachChecker } from "./providers/breach";
import type { CaptchaVerifier } from "./providers/captcha";
import type { EmailProvider } from "./providers/email/types";
import type { ConfigSigner } from "./services/configSigner";
import type { EventHub } from "./services/events";
import type { RelayControl } from "./services/relayControl";

/** Everything the API needs, injected so tests can pass fakes (ARCHITECTURE §14.1). */
export type Deps = {
  env: Env;
  db: Db;
  redis: Redis;
  clock: Clock;
  keys: KeySet;
  email: EmailProvider;
  breach: BreachChecker;
  captcha: CaptchaVerifier;
  billing: BillingProvider;
  config: ConfigSigner;
  events: EventHub;
  auth: Auth;
  ai: AiProvider;
  stt: SttProvider;
  /** Tells the phone relay to drop a revoked pairing's sockets (F-BE-11). */
  relay: RelayControl;
  /** Conductor's realtime voice: mints a session for the app to connect to (F-BE-08). */
  realtime: RealtimeProvider;
  /** What Team SSO reaches out with: the identity provider and DNS (faked in tests). */
  ssoNet: SsoNet;
};

export type SsoNet = {
  fetch: (input: string, init?: RequestInit) => Promise<Response>;
  resolveTxt: (name: string) => Promise<string[][]>;
};
