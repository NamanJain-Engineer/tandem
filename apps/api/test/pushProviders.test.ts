import { createServer, type Http2Server, type IncomingHttpHeaders } from "node:http2";
import type { AddressInfo } from "node:net";
import { exportPKCS8, exportSPKI, generateKeyPair, importSPKI, jwtVerify } from "jose";
import { afterEach, describe, expect, it } from "vitest";
import { ApnsPush, APNS_FALLBACK, FcmPush } from "../src/providers/push";

let server: Http2Server | null = null;
afterEach(() => new Promise<void>((r) => (server ? server.close(() => r()) : r())));

describe("F-RM-06 push providers", () => {
  it("F-RM-06.2 APNs: a generic alert with mutable-content, the sealed bytes as data, and a valid ES256 token", async () => {
    const { privateKey, publicKey } = await generateKeyPair("ES256", { extractable: true });
    const seen: { headers: IncomingHttpHeaders; body: string }[] = [];
    let status = 200;
    let reply = "";
    server = createServer((req, res) => {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        seen.push({ headers: req.headers, body });
        res.writeHead(status);
        res.end(reply);
      });
    });
    await new Promise<void>((r) => server!.listen(0, "127.0.0.1", () => r()));
    const port = (server.address() as AddressInfo).port;
    const apns = new ApnsPush({
      keyId: "KEY123",
      teamId: "TEAM123",
      privateKey: await exportPKCS8(privateKey),
      topic: "com.vibezeo.remote",
      origin: `http://127.0.0.1:${port}`,
    });
    const token = "a".repeat(64);
    expect(await apns.send({ platform: "apns", token, payload: "SEALED_bytes-1" })).toBe("sent");
    const [first] = seen;
    expect(first?.headers[":path"]).toBe(`/3/device/${token}`);
    expect(first?.headers["apns-topic"]).toBe("com.vibezeo.remote");
    expect(first?.headers["apns-push-type"]).toBe("alert");
    const body = JSON.parse(first?.body ?? "{}");
    expect(body).toEqual({ aps: { alert: APNS_FALLBACK, "mutable-content": 1, sound: "default" }, v: "1", p: "SEALED_bytes-1" });
    const jwt = String(first?.headers.authorization).replace(/^bearer /, "");
    const { payload, protectedHeader } = await jwtVerify(jwt, await importSPKI(await exportSPKI(publicKey), "ES256"));
    expect(protectedHeader.kid).toBe("KEY123");
    expect(payload.iss).toBe("TEAM123");
    status = 410;
    reply = JSON.stringify({ reason: "Unregistered" });
    expect(await apns.send({ platform: "apns", token, payload: "x" })).toBe("unregistered");
    status = 500;
    reply = "";
    expect(await apns.send({ platform: "apns", token, payload: "x" })).toBe("unavailable");
    expect(await apns.send({ platform: "apns", token: "not hex", payload: "x" })).toBe("unregistered");
    apns.close();
  });

  it("F-RM-06.2 FCM: a data-only high-priority message holding just the sealed bytes", async () => {
    const { privateKey } = await generateKeyPair("RS256", { extractable: true });
    const calls: { url: string; init?: RequestInit }[] = [];
    const fcm = new FcmPush({
      projectId: "vibezeo-test",
      clientEmail: "push@vibezeo-test.iam.gserviceaccount.com",
      privateKey: await exportPKCS8(privateKey),
      fetch: async (url, init) => {
        calls.push({ url, init });
        if (url.includes("oauth2")) return new Response(JSON.stringify({ access_token: "at", expires_in: 3600 }));
        return new Response("{}", { status: 200 });
      },
    });
    expect(await fcm.send({ platform: "fcm", token: "fcm-token", payload: "SEALED" })).toBe("sent");
    const send = calls.find((c) => c.url.includes("messages:send"));
    expect(send?.url).toBe("https://fcm.googleapis.com/v1/projects/vibezeo-test/messages:send");
    expect(JSON.parse(String(send?.init?.body))).toEqual({
      message: { token: "fcm-token", data: { v: "1", p: "SEALED" }, android: { priority: "HIGH", ttl: "3600s" } },
    });
  });
});
