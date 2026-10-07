/** Turnstile-compatible bot check on web sign-up and sign-in only (F-BE-10). */
export interface CaptchaVerifier {
  verify(token: string | undefined, ip: string | undefined): Promise<boolean>;
}

/** Dev and tests (TURNSTILE_SECRET unset): always passes. */
export class DevCaptcha implements CaptchaVerifier {
  async verify(): Promise<boolean> {
    return true;
  }
}

export class TurnstileCaptcha implements CaptchaVerifier {
  constructor(
    private readonly secret: string,
    private readonly fetchImpl: typeof fetch = fetch,
  ) {}

  async verify(token: string | undefined, ip: string | undefined): Promise<boolean> {
    if (!token) return false;
    const body = new URLSearchParams({ secret: this.secret, response: token });
    if (ip) body.set("remoteip", ip);
    const res = await this.fetchImpl("https://challenges.cloudflare.com/turnstile/v0/siteverify", {
      method: "POST",
      body,
    });
    const data = (await res.json()) as { success?: boolean };
    return data.success === true;
  }
}
