// D-2604: test results read from the output a command already carries.
import { describe, expect, it } from "vitest";
import { testResult } from "./tool";

describe("D-2604 test results from command output", () => {
  it.each([
    ["vitest", " Test Files  4 passed (4)\n      Tests  18 passed (18)", { passed: 18, failed: 0 }],
    ["vitest with failures", "      Tests  2 failed | 16 passed (18)", { passed: 16, failed: 2 }],
    ["jest", "Tests:       1 failed, 17 passed, 18 total", { passed: 17, failed: 1 }],
    ["cargo", "test result: ok. 12 passed; 0 failed; 0 ignored\ntest result: FAILED. 3 passed; 1 failed;", { passed: 15, failed: 1 }],
    ["pytest", "==== 9 passed, 2 failed in 0.41s ====", { passed: 9, failed: 2 }],
    ["go", "--- FAIL: TestX (0.00s)\nok  \tpkg/a\t0.01s\nFAIL\tpkg/b\t0.02s", null],
  ])("%s", (_name, output, want) => {
    expect(testResult(output)).toEqual(want);
  });

  it("is nothing for output without a test summary", () => {
    expect(testResult("Compiling foo v0.1.0\nFinished dev")).toBeNull();
  });
});
