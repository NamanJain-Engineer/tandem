import canonicalize from "canonicalize";
import { createHash, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import type { PluginClient } from "@tandem/protocol";
import { currentKey, type KeySet } from "../auth/keys";

export type SignedConfig = { payload: string; sig: string; kid: string; etag: string };

/**
 * Signs `config/remote-config.json` once at startup. `payload` is the RFC 8785
 * canonical JSON text; the signature covers its UTF-8 bytes, so the desktop
 * verifies without re-canonicalizing (ARCHITECTURE §14.10, BUILD_LOG D-19).
 */
export class ConfigSigner {
  signed: SignedConfig;
  config: RemoteConfig;

  constructor(
    private keys: KeySet,
    path = new URL("../../config/remote-config.json", import.meta.url),
    minVersion?: string,
  ) {
    const raw = JSON.parse(readFileSync(path, "utf8")) as RemoteConfig;
    if (minVersion) raw.min_app_version = minVersion;
    this.config = raw;
    this.signed = this.sign(raw);
  }

  private sign(config: RemoteConfig): SignedConfig {
    const payload = canonicalize(config);
    if (!payload) throw new Error("remote config is empty");
    const key = currentKey(this.keys.config);
    const sig = sign(null, Buffer.from(payload, "utf8"), key.privateKey).toString("base64");
    const etag = `"${createHash("sha256").update(payload).digest("hex")}"`;
    return { payload, sig, kid: key.kid, etag };
  }

  /** Test routes only: replaces the engine registry overrides and re-signs. */
  setRegistryOverrides(overrides: unknown[]): void {
    this.config = { ...this.config, registry_overrides: overrides };
    this.signed = this.sign(this.config);
  }

  /** Test routes only: the oldest app version responses name, re-signed. */
  setMinAppVersion(version: string): void {
    this.config = { ...this.config, min_app_version: version };
    this.signed = this.sign(this.config);
  }

  get minAppVersion(): string {
    return this.config.min_app_version;
  }
}

export type RemoteConfig = {
  version: number;
  min_app_version: string;
  prices: Record<string, number>;
  daily_caps: Record<string, number>;
  flags: Record<string, boolean>;
  models: Record<string, Array<{ id: string; label: string; default?: boolean }>>;
  /** The Claude model each hosted AI feature uses (D-103). */
  ai_models?: Record<string, string>;
  registry_overrides: unknown[];
  /** Per plugin: Vibezeo's OAuth client with that vendor, and an off switch (F-PL-09). */
  plugins: Record<string, PluginClient>;
  telemetry_allowlist_version: number;
};
