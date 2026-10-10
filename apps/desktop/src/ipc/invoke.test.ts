import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
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

  it("§5.2 an aborted signal cancels the call: before it starts and while it runs", async () => {
    let calls = 0;
    setInvokeBackend(() => {
      calls += 1;
      return new Promise(() => undefined);
    });
    const done = new AbortController();
    done.abort();
    await expect(invoke("slow", undefined, { signal: done.signal })).rejects.toMatchObject({ code: "cancelled" });
    expect(calls).toBe(0);

    const later = new AbortController();
    const pending = invoke("slow", undefined, { signal: later.signal });
    await new Promise((r) => setTimeout(r, 0)); // the call is in flight
    expect(calls).toBe(1);
    later.abort();
    await expect(pending).rejects.toMatchObject({ code: "cancelled", retryable: false });
    expect(calls).toBe(1);
  });

  it("§5.2 a result that arrives after cancelling is dropped, not delivered", async () => {
    let finish: (v: unknown) => void = () => undefined;
    setInvokeBackend(() => new Promise((resolve) => (finish = resolve)));
    const ctl = new AbortController();
    const pending = invoke("slow", undefined, { signal: ctl.signal });
    await new Promise((r) => setTimeout(r, 0));
    ctl.abort();
    finish({ late: true });
    await expect(pending).rejects.toMatchObject({ code: "cancelled" });
  });

  it("§5.2 every command goes through this wrapper, so every call has the timeout", () => {
    const root = join(import.meta.dirname, "..");
    const files: string[] = [];
    const walk = (dir: string) => {
      for (const name of readdirSync(dir)) {
        const path = join(dir, name);
        if (statSync(path).isDirectory()) walk(path);
        else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) files.push(path);
      }
    };
    walk(root);
    const direct = files.filter((f) => {
      const src = readFileSync(f, "utf8");
      return /import\s*\{[^}]*\binvoke\b[^}]*\}\s*from\s*"@tauri-apps\/api\/core"/.test(src) || /core\.invoke\(/.test(src);
    });
    expect(direct.map((f) => f.slice(root.length + 1))).toEqual(["ipc/invoke.ts"]);
  });
});
