import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { b64url, fromB64url } from "./protocol";
import { openPushPayload, pushVectors, sealPush } from "./push";

const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/tandem-core/tests/fixtures/remote_push_v1.json", import.meta.url), "utf8"),
);

describe("F-RM-06 sealed push alerts", () => {
  it("F-RM-06.2 matches the committed vectors (and so the Rust side)", () => {
    expect(pushVectors()).toEqual(fixture);
    const { seq, alert } = openPushPayload(fixture.pid, fromB64url(fixture.key), fixture.frame);
    expect(seq).toBe(7n);
    expect(alert.body).toBe("Run pnpm test");
  });

  it("F-RM-06.2 refuses another pairing, another key or a changed byte", () => {
    const key = new Uint8Array(32).fill(1);
    const frame = sealPush("pr_a", key, 3n, new TextEncoder().encode('{"x":1}'));
    expect(() => openPushPayload("pr_b", key, b64url(frame))).toThrow();
    expect(() => openPushPayload("pr_a", new Uint8Array(32), b64url(frame))).toThrow();
    const bad = frame.slice();
    bad[bad.length - 1] ^= 1;
    expect(() => openPushPayload("pr_a", key, b64url(bad))).toThrow();
    expect(b64url(frame).length).toBeLessThan(3000);
  });
});
