// Push for the phone companion (F-RM-06, D-2020). The API forwards opaque
// bytes the desktop sealed for the phone; providers never see readable text.

export type PushPlatform = "fcm" | "apns";

export type PushMessage = {
  platform: PushPlatform;
  /** The phone's push token, which the desktop learned over the encrypted channel. */
  token: string;
  /** base64url of the sealed payload (only the phone can open it). */
  payload: string;
};

export type PushResult = "sent" | "unregistered" | "unavailable";

export interface PushProvider {
  readonly name: string;
  send(message: PushMessage): Promise<PushResult>;
}
