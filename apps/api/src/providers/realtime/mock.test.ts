import { describe, expect, it } from "vitest";
import { MockRealtime } from "./mock";

describe("MockRealtime (F-CN-01, D-1102)", () => {
  it("F-CN-01 points at the API's dev path by default", async () => {
    const m = await new MockRealtime("http://127.0.0.1:8787/").mint({ instructions: "", maxSeconds: 1800 });
    expect(m.wsUrl).toBe(`ws://127.0.0.1:8787/v1/dev/realtime?token=${encodeURIComponent(m.token)}`);
  });

  it("F-CN-01 points at scripts/mock-realtime.ts when REALTIME_MOCK_URL is set", async () => {
    const m = await new MockRealtime("http://127.0.0.1:8787", "ws://127.0.0.1:8794/realtime").mint({
      instructions: "",
      maxSeconds: 1800,
    });
    expect(m.wsUrl).toBe(`ws://127.0.0.1:8794/realtime?token=${encodeURIComponent(m.token)}`);
    expect(m.token).toMatch(/^rt_mock_/);
  });
});
