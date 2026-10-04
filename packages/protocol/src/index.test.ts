import { describe, expect, it } from "vitest";
import type { Health, Problem, SyncPage } from "./index";

describe("protocol", () => {
  it("health, problem, and sync page shapes are assignable", () => {
    const health: Health = { ok: true, version: "0.1.0" };
    const problem: Problem = { type: "about:blank", title: "Not Found", status: 404 };
    const page: SyncPage = { items: [], nextCursor: null };
    expect(health.ok).toBe(true);
    expect(problem.status).toBe(404);
    expect(page.nextCursor).toBeNull();
  });
});

