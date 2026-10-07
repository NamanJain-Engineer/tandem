import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

export interface BreachChecker {
  /** True when the password appears in known breaches. */
  isBreached(password: string): Promise<boolean>;
}

/** Dev and tests: a local list (`src/auth/breached.txt`, contains "password123"). */
export class OfflineBreachList implements BreachChecker {
  private readonly hashes: Set<string>;

  constructor(path = new URL("../../auth/breached.txt", import.meta.url)) {
    const words = readFileSync(path, "utf8")
      .split("\n")
      .map((w) => w.trim())
      .filter(Boolean);
    this.hashes = new Set(words.map((w) => createHash("sha1").update(w).digest("hex").toUpperCase()));
  }

  async isBreached(password: string): Promise<boolean> {
    return this.hashes.has(createHash("sha1").update(password).digest("hex").toUpperCase());
  }
}

/** Production: HIBP k-anonymity range API. Only the first 5 hex chars of the SHA-1 leave the server. */
export class HibpBreachChecker implements BreachChecker {
  constructor(private readonly fetchImpl: typeof fetch = fetch) {}

  async isBreached(password: string): Promise<boolean> {
    const hash = createHash("sha1").update(password).digest("hex").toUpperCase();
    const prefix = hash.slice(0, 5);
    const suffix = hash.slice(5);
    try {
      const res = await this.fetchImpl(`https://api.pwnedpasswords.com/range/${prefix}`, {
        headers: { "Add-Padding": "true" },
      });
      if (!res.ok) return false;
      const body = await res.text();
      return body
        .split("\n")
        .some((line) => line.split(":")[0] === suffix && line.split(":")[1]?.trim() !== "0");
    } catch {
      // Fail open: an outage of the breach API must not block sign-up.
      return false;
    }
  }
}
