import { describe, expect, it } from "vitest";
import type { PhoneReview, PhoneReviewItem } from "@tandem/protocol";
import { checkText, lineKind, reviewLines, ruleText, statText } from "./review";

const item: PhoneReviewItem = {
  key: "pane:p1",
  title: "Ada",
  engine: null,
  workspace: "shop",
  branch: null,
  finishedAt: 0,
  closing: null,
  files: 1,
  additions: 3,
  deletions: 0,
  risks: [],
  check: { state: "failed", exit: 1, output: "" },
  changed: false,
};

describe("the phone's Review tab (F-CN-09)", () => {
  it("F-CN-09.5 says nothing waits, or the item's stats, check, labels and what stays on the computer", () => {
    expect(reviewLines({ item: null, files: [], position: 0, total: 0, omitted: 0 })).toEqual({
      empty: true,
    });
    const r: PhoneReview = {
      item,
      files: [
        {
          path: ".env",
          change: "modified",
          additions: 1,
          deletions: 0,
          patch: "",
          truncated: false,
          withheld: true,
        },
      ],
      position: 1,
      total: 1,
      omitted: 2,
    };
    expect(reviewLines(r)).toEqual({
      empty: false,
      withheld: [".env: not shown on the phone, since .env files hold secrets."],
      omitted: "2 more files are only on your computer.",
    });
    expect(statText(item)).toBe("1 file · +3 −0");
    expect(statText({ ...item, files: 2 })).toBe("2 files · +3 −0");
    expect(checkText(item)).toBe("Check failed (exit 1)");
    expect(checkText({ ...item, check: null })).toBeNull();
    expect(ruleText("migrations")).toBe("Touches migrations");
  });

  it("F-CN-09.5 diff headers aren't coloured as changes", () => {
    expect(["--- a/x", "+++ b/x", "@@ -1 +1 @@", "+new", "-old", " same"].map(lineKind)).toEqual([
      "meta",
      "meta",
      "meta",
      "add",
      "del",
      "context",
    ]);
  });
});
