// Firebase Cloud Messaging (HTTP v1), free. A data-only, high-priority
// message carries the sealed payload; the Android app opens it and shows a
// local notification (F-RM-06). Credentials: a service account with the FCM
// role (HUMAN_TODO "Push notifications").
import { importPKCS8, SignJWT } from "jose";
import type { PushMessage, PushProvider, PushResult } from "./types";

export type FcmConfig = {
  projectId: string;
  clientEmail: string;
  privateKey: string;
  fetch?: (input: string, init?: RequestInit) => Promise<Response>;
  now?: () => number;
};

const SCOPE = "https://www.googleapis.com/auth/firebase.messaging";
const TOKEN_URL = "https://oauth2.googleapis.com/token";

export class FcmPush implements PushProvider {
  readonly name = "fcm";
  private access: { token: string; until: number } | null = null;
  private readonly fetch: (input: string, init?: RequestInit) => Promise<Response>;
  private readonly now: () => number;

  constructor(private readonly config: FcmConfig) {
    this.fetch = config.fetch ?? ((i, init) => fetch(i, init));
    this.now = config.now ?? (() => Date.now());
  }

  private async accessToken(): Promise<string> {
    if (this.access && this.access.until > this.now() + 60_000) return this.access.token;
    const key = await importPKCS8(this.config.privateKey.replace(/\\n/g, "\n"), "RS256");
    const iat = Math.floor(this.now() / 1000);
    const assertion = await new SignJWT({ scope: SCOPE })
      .setProtectedHeader({ alg: "RS256", typ: "JWT" })
      .setIssuer(this.config.clientEmail)
      .setAudience(TOKEN_URL)
      .setIssuedAt(iat)
      .setExpirationTime(iat + 3600)
      .sign(key);
    const res = await this.fetch(TOKEN_URL, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer", assertion }).toString(),
    });
    if (!res.ok) throw new Error(`fcm auth ${res.status}`);
    const body = (await res.json()) as { access_token: string; expires_in: number };
    this.access = { token: body.access_token, until: this.now() + body.expires_in * 1000 };
    return body.access_token;
  }

  async send(message: PushMessage): Promise<PushResult> {
    const token = await this.accessToken();
    const res = await this.fetch(`https://fcm.googleapis.com/v1/projects/${this.config.projectId}/messages:send`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify({
        message: {
          token: message.token,
          data: { v: "1", p: message.payload },
          android: { priority: "HIGH", ttl: "3600s" },
        },
      }),
    });
    if (res.ok) return "sent";
    if (res.status === 404 || res.status === 400) {
      const text = await res.text();
      if (/UNREGISTERED|registration-token-not-registered|INVALID_ARGUMENT/.test(text)) return "unregistered";
    }
    return "unavailable";
  }
}
