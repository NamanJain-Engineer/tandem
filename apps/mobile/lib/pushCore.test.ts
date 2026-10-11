import { describe, expect, it } from "vitest";
import { b64url, encodeJson, fromB64url } from "@tandem/remote/protocol";
import { sealPush } from "@tandem/remote/push";
import { loadPushSecret, newPushSecret, openAlert, payloadOf, savePushSecret } from "./pushCore";

const random = (n: number) => crypto.getRandomValues(new Uint8Array(n));

function sealed(pid: string, key: string, seq: bigint, title = "Approval needed") {
  return b64url(sealPush(pid, fromB64url(key), seq, encodeJson({ kind: "approval", id: "ap_1", title, body: "Run pnpm test", at: 1 })));
}

describe("F-RM-06 push on the phone", () => {
  it("F-RM-06.2 opens an alert sealed for this pairing, once", () => {
    const secret = newPushSecret("pr_push1", random);
    const payload = sealed("pr_push1", secret.key, 3n);
    const opened = openAlert(secret, payload);
    expect(opened?.alert.title).toBe("Approval needed");
    expect(opened?.seq).toBe(3n);
    expect(openAlert({ ...secret, lastSeq: "3" }, payload)).toBeNull();
    expect(openAlert(newPushSecret("pr_push1", random), payload)).toBeNull();
    expect(openAlert({ ...secret, pid: "pr_other" }, payload)).toBeNull();
  });

  it("finds the payload in the shapes FCM data arrives in", () => {
    expect(payloadOf({ v: "1", p: "abc" })).toBe("abc");
    expect(payloadOf({ data: { v: "1", p: "abc" } })).toBe("abc");
    expect(payloadOf({ dataString: JSON.stringify({ v: "1", p: "abc" }) })).toBe("abc");
    expect(payloadOf({ v: "2", p: "abc" })).toBeNull();
    expect(payloadOf("nonsense")).toBeNull();
  });

  it("F-RM-05.5 the alert key is kept in secure storage only", async () => {
    const map = new Map<string, string>();
    const kv = {
      getItemAsync: async (k: string) => map.get(k) ?? null,
      setItemAsync: async (k: string, v: string) => void map.set(k, v),
      deleteItemAsync: async (k: string) => void map.delete(k),
    };
    const s = newPushSecret("pr_x", random);
    await savePushSecret(kv, s);
    expect(await loadPushSecret(kv)).toEqual(s);
    expect(fromB64url(s.key).length).toBe(32);
  });
});
