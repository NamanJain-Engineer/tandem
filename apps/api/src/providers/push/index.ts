import type { Env } from "../../env";
import { ApnsPush, ApnsPushStub } from "./apns";
import { FcmPush } from "./fcm";
import { MockPush } from "./mock";
import type { PushMessage, PushProvider, PushResult } from "./types";

export { ApnsPush, ApnsPushStub, apnsBody, APNS_FALLBACK } from "./apns";
export { FcmPush } from "./fcm";
export { MockPush } from "./mock";
export * from "./types";

/** Routes each message to its platform's provider. */
export class PushRouter implements PushProvider {
  readonly name = "router";
  constructor(private readonly by: Record<PushMessage["platform"], PushProvider>) {}
  send(message: PushMessage): Promise<PushResult> {
    return this.by[message.platform].send(message);
  }
}

/**
 * `PUSH_PROVIDER=live` sends for real: FCM with FCM_PROJECT_ID,
 * FCM_CLIENT_EMAIL and FCM_PRIVATE_KEY; APNs with APNS_KEY_ID, APNS_TEAM_ID,
 * APNS_PRIVATE_KEY and APNS_TOPIC (each platform is unavailable without its
 * credentials). Anything else is MockPush.
 */
export function selectPush(env: Env): PushProvider {
  if (env.PUSH_PROVIDER === "live") {
    const fcm =
      env.FCM_PROJECT_ID && env.FCM_CLIENT_EMAIL && env.FCM_PRIVATE_KEY
        ? new FcmPush({ projectId: env.FCM_PROJECT_ID, clientEmail: env.FCM_CLIENT_EMAIL, privateKey: env.FCM_PRIVATE_KEY })
        : new ApnsPushStub();
    const apns =
      env.APNS_KEY_ID && env.APNS_TEAM_ID && env.APNS_PRIVATE_KEY
        ? new ApnsPush({
            keyId: env.APNS_KEY_ID,
            teamId: env.APNS_TEAM_ID,
            privateKey: env.APNS_PRIVATE_KEY,
            topic: env.APNS_TOPIC,
          })
        : new ApnsPushStub();
    return new PushRouter({ fcm, apns });
  }
  const mock = new MockPush();
  return new PushRouter({ fcm: mock, apns: mock });
}
