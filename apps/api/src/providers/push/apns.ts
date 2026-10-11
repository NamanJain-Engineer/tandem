// Apple Push Notification service (F-RM-06, D-2022). Each push is a generic
// visible alert with `mutable-content`, carrying the sealed payload as data;
// the app's Notification Service Extension opens it with the key in the
// shared keychain and replaces the text before iOS shows it. Nothing
// readable leaves the desktop. Needs the paid Apple account's APNs auth key
// (HUMAN_TODO "Push notifications"); until then `ApnsPushStub` answers.
import { connect, type ClientHttp2Session } from "node:http2";
import { importPKCS8, SignJWT } from "jose";
import type { PushMessage, PushProvider, PushResult } from "./types";

export type ApnsConfig = {
  keyId: string;
  teamId: string;
  /** The .p8 key's PEM. */
  privateKey: string;
  /** The app's bundle id, `com.vibezeo.remote`. */
  topic: string;
  /** `https://api.push.apple.com` (production) unless tests point elsewhere. */
  origin?: string;
  now?: () => number;
};

/** The alert iOS shows if the extension can't open the payload. */
export const APNS_FALLBACK = { title: "Vibezeo", body: "Something on your computer needs you." };

export function apnsBody(payload: string): string {
  return JSON.stringify({
    aps: { alert: APNS_FALLBACK, "mutable-content": 1, sound: "default" },
    v: "1",
    p: payload,
  });
}

export class ApnsPush implements PushProvider {
  readonly name = "apns";
  private jwt: { token: string; at: number } | null = null;
  private session: ClientHttp2Session | null = null;
  private readonly now: () => number;

  constructor(private readonly config: ApnsConfig) {
    this.now = config.now ?? (() => Date.now());
  }

  /** Apple accepts a provider token for an hour; it's renewed after 50 minutes. */
  private async token(): Promise<string> {
    if (this.jwt && this.now() - this.jwt.at < 50 * 60_000) return this.jwt.token;
    const key = await importPKCS8(this.config.privateKey.replace(/\\n/g, "\n"), "ES256");
    const token = await new SignJWT({})
      .setProtectedHeader({ alg: "ES256", kid: this.config.keyId })
      .setIssuer(this.config.teamId)
      .setIssuedAt(Math.floor(this.now() / 1000))
      .sign(key);
    this.jwt = { token, at: this.now() };
    return token;
  }

  private client(): ClientHttp2Session {
    if (!this.session || this.session.closed || this.session.destroyed) {
      this.session = connect(this.config.origin ?? "https://api.push.apple.com");
      this.session.on("error", () => undefined);
    }
    return this.session;
  }

  close(): void {
    this.session?.close();
  }

  async send(message: PushMessage): Promise<PushResult> {
    if (!/^[0-9a-fA-F]{32,200}$/.test(message.token)) return "unregistered";
    const auth = await this.token();
    const body = apnsBody(message.payload);
    return new Promise<PushResult>((resolve) => {
      const req = this.client().request({
        ":method": "POST",
        ":path": `/3/device/${message.token}`,
        authorization: `bearer ${auth}`,
        "apns-topic": this.config.topic,
        "apns-push-type": "alert",
        "apns-priority": "10",
        "apns-expiration": String(Math.floor(this.now() / 1000) + 3600),
        "content-type": "application/json",
      });
      let status = 0;
      let text = "";
      req.setEncoding("utf8");
      req.on("response", (h) => (status = Number(h[":status"])));
      req.on("data", (c: string) => (text += c));
      req.on("end", () => {
        if (status === 200) resolve("sent");
        else if (status === 410 || /BadDeviceToken|Unregistered|DeviceTokenNotForTopic/.test(text)) resolve("unregistered");
        else resolve("unavailable");
      });
      req.on("error", () => resolve("unavailable"));
      req.end(body);
    });
  }
}

/** Until the Apple account exists: iOS push is unavailable. */
export class ApnsPushStub implements PushProvider {
  readonly name = "apns-unconfigured";

  async send(_message: PushMessage): Promise<PushResult> {
    return "unavailable";
  }
}
