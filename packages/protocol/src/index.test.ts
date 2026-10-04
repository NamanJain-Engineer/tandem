import { describe, expect, it } from "vitest";
import type { Health, Problem } from "./index";

describe("protocol", () => {
  it("health and problem shapes are assignable", () => {
    const health: Health = { ok: true, version: "0.1.0" };
    const problem: Problem = { type: "about:blank", title: "Not Found", status: 404 };
    expect(health.ok).toBe(true);
    expect(problem.status).toBe(404);
  });
});
