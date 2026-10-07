import { createHash, randomBytes } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { json, makeTestApp, signUpVerified, type TestEnv } from "./helpers";

let t: TestEnv;
beforeEach(async () => {
  t = await makeTestApp();
});
afterEach(async () => {
  await t.close();
});

function pkce() {
  const verifier = randomBytes(48).toString("base64url");
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  return { verifier, challenge, state: randomBytes(24).toString("base64url"), fingerprint: "f".repeat(64) };
}

async function authorize(redirectUri = "http://127.0.0.1:53111/callback") {
  const { jar } = await signUpVerified(t, "desk@tandem.test");
  const p = pkce();
  const res = await json(t, "POST", "/v1/auth/desktop/authorize", {
    jar,
    body: {
      challenge: p.challenge,
      state: p.state,
      redirect_uri: redirectUri,
      device_fingerprint: p.fingerprint,
      device_name: "Work laptop",
      platform: "linux",
    },
  });
  expect(res.status).toBe(200);
  const { code } = (await res.json()) as { code: string };
  return { jar, code, ...p };
}

describe("F-BE-02 desktop sign-in (server side)", () => {
  it("authorize → exchange returns an access token and a refresh token", async () => {
    const a = await authorize();
    const res = await json(t, "POST", "/v1/auth/desktop/exchange", {
      body: { code: a.code, verifier: a.verifier, device_fingerprint: a.fingerprint },
    });
    expect(res.status).toBe(200);
    const tokens = (await res.json()) as { access_token: string; token_type: string; expires_in: number };
    expect(tokens).toMatchObject({ token_type: "Bearer", expires_in: 3600 });
    const me = await json(t, "GET", "/v1/me", { bearer: tokens.access_token });
    expect(me.status).toBe(200);
  });

  it("F-BE-02.2 an exchange with the wrong verifier returns 400", async () => {
    const a = await authorize();
    const res = await json(t, "POST", "/v1/auth/desktop/exchange", {
      body: {
        code: a.code,
        verifier: randomBytes(48).toString("base64url"),
        device_fingerprint: a.fingerprint,
      },
    });
    expect(res.status).toBe(400);
    expect(await res.json()).toMatchObject({ error: "invalid_grant" });
  });

  it("codes are single-use and expire after 5 minutes", async () => {
    const a = await authorize();
    const body = { code: a.code, verifier: a.verifier, device_fingerprint: a.fingerprint };
    expect((await json(t, "POST", "/v1/auth/desktop/exchange", { body })).status).toBe(200);
    expect((await json(t, "POST", "/v1/auth/desktop/exchange", { body })).status).toBe(400);
    const b = await authorize();
    t.clock.advance(5 * 60_000 + 1000);
    const late = await json(t, "POST", "/v1/auth/desktop/exchange", {
      body: { code: b.code, verifier: b.verifier, device_fingerprint: b.fingerprint },
    });
    expect(late.status).toBe(400);
  });

  it("only the app's loopback and tandem:// redirect URIs are allowed", async () => {
    const { jar } = await signUpVerified(t, "redir@tandem.test");
    const p = pkce();
    for (const uri of ["https://evil.example/callback", "http://127.0.0.1/callback", "tandem://evil"]) {
      const res = await json(t, "POST", "/v1/auth/desktop/authorize", {
        jar,
        body: {
          challenge: p.challenge,
          state: p.state,
          redirect_uri: uri,
          device_fingerprint: p.fingerprint,
          platform: "linux",
        },
      });
      expect(res.status, uri).toBe(422);
    }
    const ok = await json(t, "POST", "/v1/auth/desktop/authorize", {
      jar,
      body: {
        challenge: p.challenge,
        state: p.state,
        redirect_uri: "tandem-dev://auth/callback",
        device_fingerprint: p.fingerprint,
        platform: "macos",
      },
    });
    expect(ok.status).toBe(200);
  });

  it("F-BE-02.3 reusing a rotated refresh token revokes the whole family", async () => {
    const a = await authorize();
    const first = (await (
      await json(t, "POST", "/v1/auth/desktop/exchange", {
        body: { code: a.code, verifier: a.verifier, device_fingerprint: a.fingerprint },
      })
    ).json()) as { refresh_token: string };
    const second = await json(t, "POST", "/v1/auth/desktop/refresh", {
      body: { refresh_token: first.refresh_token },
    });
    expect(second.status).toBe(200);
    const rotated = (await second.json()) as { refresh_token: string };
    const reuse = await json(t, "POST", "/v1/auth/desktop/refresh", {
      body: { refresh_token: first.refresh_token },
    });
    expect(reuse.status).toBe(401);
    const after = await json(t, "POST", "/v1/auth/desktop/refresh", {
      body: { refresh_token: rotated.refresh_token },
    });
    expect(after.status).toBe(401);
  });

  it("F-WB-05.1 revoking a device from the portal makes its next refresh fail", async () => {
    const a = await authorize();
    const tokens = (await (
      await json(t, "POST", "/v1/auth/desktop/exchange", {
        body: { code: a.code, verifier: a.verifier, device_fingerprint: a.fingerprint },
      })
    ).json()) as { refresh_token: string };
    const list = (await (await json(t, "GET", "/v1/me/sessions", { jar: a.jar })).json()) as Array<{
      id: string;
      deviceName: string;
    }>;
    expect(list).toHaveLength(1);
    expect(list[0]?.deviceName).toBe("Work laptop");
    const del = await json(t, "DELETE", `/v1/me/sessions/${list[0]?.id}`, { jar: a.jar });
    expect(del.status).toBe(204);
    const refresh = await json(t, "POST", "/v1/auth/desktop/refresh", {
      body: { refresh_token: tokens.refresh_token },
    });
    expect(refresh.status).toBe(401);
  });
});
