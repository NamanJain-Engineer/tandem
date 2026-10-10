import { describe, expect, it } from "vitest";
import { parseArgs } from "./EnginesPage";

describe("extra launch arguments (UI-105)", () => {
  it("are a shell-word list with quotes", () => {
    expect(parseArgs("--model sonnet  --verbose")).toEqual(["--model", "sonnet", "--verbose"]);
    expect(parseArgs(`--append-system-prompt "be brief" '' x`)).toEqual([
      "--append-system-prompt",
      "be brief",
      "",
      "x",
    ]);
    expect(parseArgs("")).toEqual([]);
    expect(parseArgs(`--name "open`)).toBeNull();
  });
});
