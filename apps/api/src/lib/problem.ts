import type { Context } from "hono";
import type { ContentfulStatusCode } from "hono/utils/http-status";
import type { Problem } from "@tandem/protocol";

/** An error that becomes an RFC 9457 Problem Details response. */
export class ApiError extends Error {
  constructor(
    public readonly status: ContentfulStatusCode,
    public readonly code: string,
    public readonly title: string,
    public readonly detail?: string,
    public readonly extra: Record<string, unknown> = {},
  ) {
    super(detail ?? title);
    this.name = "ApiError";
  }
}

export function problemBody(status: number, code: string, title: string, detail?: string): Problem {
  return { type: "about:blank", title, status, code, ...(detail ? { detail } : {}) };
}

export function problemResponse(c: Context, err: ApiError): Response {
  const body = {
    ...problemBody(err.status, err.code, err.title, err.detail),
    ...err.extra,
    instance: c.req.path,
  };
  return c.body(JSON.stringify(body), err.status, { "content-type": "application/problem+json" });
}
