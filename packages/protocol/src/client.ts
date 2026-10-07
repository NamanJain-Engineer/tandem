import type { Health, Problem } from "./api";

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly problem: Problem,
  ) {
    super(problem.detail ?? problem.title);
    this.name = "ApiError";
  }
}

export type ClientOptions = {
  baseUrl: string;
  /** Returns the current bearer token, if signed in. */
  token?: () => string | undefined;
  /** Sent as x-tandem-app / x-tandem-version / x-tandem-platform. */
  app?: { name: string; version: string; platform: string };
  fetch?: typeof fetch;
};

/** Minimal typed client shared by the desktop UI and the web app. */
export function createClient(options: ClientOptions) {
  const doFetch = options.fetch ?? fetch;

  async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { accept: "application/json" };
    if (body !== undefined) headers["content-type"] = "application/json";
    const token = options.token?.();
    if (token) headers.authorization = `Bearer ${token}`;
    if (options.app) {
      headers["x-tandem-app"] = options.app.name;
      headers["x-tandem-version"] = options.app.version;
      headers["x-tandem-platform"] = options.app.platform;
    }
    const res = await doFetch(new URL(path, options.baseUrl), {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) {
      const problem = (await res.json().catch(() => ({
        type: "about:blank",
        title: res.statusText,
        status: res.status,
      }))) as Problem;
      throw new ApiError(res.status, problem);
    }
    if (res.status === 204) return undefined as T;
    return (await res.json()) as T;
  }

  return {
    request,
    health: () => request<Health>("GET", "/v1/health"),
  };
}

export type ApiClient = ReturnType<typeof createClient>;
