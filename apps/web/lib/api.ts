"use client";

export type Problem = { status: number; title: string; code?: string; detail?: string; message?: string };

function csrfToken(): string | undefined {
  return document.cookie
    .split("; ")
    .find((c) => c.startsWith("tandem_csrf="))
    ?.split("=")[1];
}

export class ApiFailure extends Error {
  constructor(
    readonly status: number,
    readonly problem: Problem,
  ) {
    super(problem.detail ?? problem.message ?? problem.title);
  }
}

/** Calls the API through the same-origin /v1 rewrite, echoing the CSRF cookie. */
export async function api<T>(path: string, init: { method?: string; body?: unknown } = {}): Promise<T> {
  const method = init.method ?? "GET";
  if (method !== "GET" && !csrfToken()) {
    // First request of a session: fetch once so the API sets the CSRF cookie.
    await fetch("/v1/health", { credentials: "include" });
  }
  const headers: Record<string, string> = { accept: "application/json" };
  if (init.body !== undefined) headers["content-type"] = "application/json";
  const token = csrfToken();
  if (token) headers["x-csrf-token"] = token;
  const res = await fetch(path, {
    method,
    headers,
    credentials: "include",
    body: init.body === undefined ? undefined : JSON.stringify(init.body),
  });
  if (!res.ok) {
    const problem = (await res
      .json()
      .catch(() => ({ status: res.status, title: res.statusText }))) as Problem;
    throw new ApiFailure(res.status, problem);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export type Me = {
  user: { id: string; email: string; name: string; emailVerified: boolean; role: string };
  plan: "free" | "pro" | "team";
  status: string;
  paid_until: string | null;
  cancel_at_period_end: boolean;
  limits: { panes: number | null; creditPacks: boolean; admin: boolean };
  credits: {
    monthly: number;
    purchased: number;
    resets_at: string | null;
    cap: number | null;
    spent: number | null;
  };
  org: { id: string; name: string; role: "owner" | "admin" | "member" } | null;
};

export function formatDate(iso: string | null): string {
  if (!iso) return "";
  return new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric" }).format(
    new Date(iso),
  );
}
