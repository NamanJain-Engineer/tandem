import { afterEach, describe, expect, it } from "vitest";
import { invoke, setInvokeBackend } from "./invoke";

afterEach(() => setInvokeBackend(null));

describe("invoke", () => {
  it("returns the command result", async () => {
    setInvokeBackend(async (cmd) => ({ cmd }));
    await expect(invoke("app_ready")).resolves.toEqual({ cmd: "app_ready" });
  });

  it("rejects with a typed IpcError on timeout", async () => {
    setInvokeBackend(() => new Promise(() => undefined));
    await expect(invoke("slow", undefined, { timeoutMs: 10 })).rejects.toMatchObject({
      code: "timeout",
      retryable: true,
    });
  });

  it("maps backend errors", async () => {
    setInvokeBackend(async () => {
      throw { code: "not_found", message: "gone", stage: "store", hint: null, retryable: false };
    });
    await expect(invoke("x")).rejects.toMatchObject({ code: "not_found", stage: "store" });
  });
});
