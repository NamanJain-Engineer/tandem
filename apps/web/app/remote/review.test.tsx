import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PhoneReview } from "@tandem/remote/messages";
import { ReviewBody } from "./ReviewBody";

const review: PhoneReview = {
  item: {
    key: "pane:p1",
    title: "Ada",
    engine: "Claude Code",
    workspace: "shop",
    branch: "tandem/fix-auth",
    finishedAt: 0,
    closing: "Handled expired tokens.",
    files: 2,
    additions: 4,
    deletions: 1,
    risks: [{ rule: "migrations", files: ["db/migrations/1.sql"] }],
    check: { state: "failed", exit: 1, output: "" },
    changed: false,
  },
  files: [
    {
      path: "db/migrations/1.sql",
      change: "added",
      additions: 1,
      deletions: 0,
      patch:
        "diff --git a/db/migrations/1.sql b/db/migrations/1.sql\n--- /dev/null\n+++ b/db/migrations/1.sql\n@@ -0,0 +1 @@\n+create table t;\n",
      truncated: true,
      withheld: false,
    },
    {
      path: ".env.local",
      change: "modified",
      additions: 1,
      deletions: 0,
      patch: "",
      truncated: false,
      withheld: true,
    },
  ],
  position: 1,
  total: 2,
  omitted: 3,
};

describe("the phone's Review tab (F-CN-09)", () => {
  it("F-CN-09.5 shows the item, its labels, check and diff read only, and what stays on the computer", () => {
    const html = renderToStaticMarkup(<ReviewBody review={review} />);
    expect(html).toContain("Ada");
    expect(html).toContain("1 of 2");
    expect(html).toContain("Touches migrations");
    expect(html).toContain("Check failed (exit 1)");
    expect(html).toContain("create table t;");
    expect(html).toContain("The rest of this file");
    expect(html).toContain(".env.local: not shown on the phone");
    expect(html).toContain("3 more files are only on your computer.");
    // Read only: nothing to approve, send back or merge from here.
    expect(html).not.toMatch(/<button[^>]*>(Approve|Merge|Send back)/);
  });

  it("F-CN-09.5 says when nothing waits", () => {
    const html = renderToStaticMarkup(
      <ReviewBody review={{ item: null, files: [], position: 0, total: 0, omitted: 0 }} />,
    );
    expect(html).toContain("Nothing to review");
  });
});
