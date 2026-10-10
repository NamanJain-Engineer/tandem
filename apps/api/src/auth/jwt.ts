// Access JWTs (1 h) and entitlement tokens (24 h), both EdDSA via jose.
import { SignJWT, jwtVerify, decodeProtectedHeader } from "jose";
import type { Limits, Plan, PlanStatus } from "@tandem/protocol/plans";
import type { OrgPolicy } from "@tandem/protocol/team";
import { currentKey, type KeySet } from "./keys";

export const ISSUER = "tandem-api";
export const ACCESS_TTL_SECONDS = 3600;
export const ENTITLEMENT_TTL_SECONDS = 24 * 3600;

export type AccessClaims = { sub: string; sid: string };

export async function signAccessToken(keys: KeySet, claims: AccessClaims, nowSec: number): Promise<string> {
  const key = currentKey(keys.access);
  return new SignJWT({ sid: claims.sid })
    .setProtectedHeader({ alg: "EdDSA", kid: key.kid, typ: "JWT" })
    .setSubject(claims.sub)
    .setIssuer(ISSUER)
    .setAudience("tandem-api")
    .setIssuedAt(nowSec)
    .setExpirationTime(nowSec + ACCESS_TTL_SECONDS)
    .sign(key.privateKey);
}

export async function verifyAccessToken(
  keys: KeySet,
  token: string,
  nowSec: number,
): Promise<AccessClaims | null> {
  try {
    const { kid } = decodeProtectedHeader(token);
    const key = keys.access.find((k) => k.kid === kid);
    if (!key) return null;
    const { payload } = await jwtVerify(token, key.publicKey, {
      issuer: ISSUER,
      audience: "tandem-api",
      algorithms: ["EdDSA"],
      currentDate: new Date(nowSec * 1000),
    });
    if (typeof payload.sub !== "string" || typeof payload.sid !== "string") return null;
    return { sub: payload.sub, sid: payload.sid };
  } catch {
    return null;
  }
}

export type EntitlementPayload = {
  sub: string;
  plan: Plan;
  status: PlanStatus;
  limits: Limits;
  policy: OrgPolicy | null;
};

export async function signEntitlement(
  keys: KeySet,
  payload: EntitlementPayload,
  nowSec: number,
): Promise<string> {
  const key = currentKey(keys.entitlement);
  return new SignJWT({
    plan: payload.plan,
    status: payload.status,
    limits: payload.limits,
    policy: payload.policy,
  })
    .setProtectedHeader({ alg: "EdDSA", kid: key.kid, typ: "JWT" })
    .setSubject(payload.sub)
    .setIssuer(ISSUER)
    .setAudience("tandem-desktop")
    .setIssuedAt(nowSec)
    .setExpirationTime(nowSec + ENTITLEMENT_TTL_SECONDS)
    .sign(key.privateKey);
}
