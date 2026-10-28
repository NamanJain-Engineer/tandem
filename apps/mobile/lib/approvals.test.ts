import { describe, expect, it } from "vitest";
import type { PhoneApproval } from "@tandem/protocol";
import { approvalActions } from "./approvals";

const base: PhoneApproval = {
  id: "ap_1",
  summary: "Run `pnpm test`",
  detail: "pnpm test",
  source: "Thread",
  queued: false,
  deskOnly: false,
  createdAt: 1,
};

describe("F-TL-12.11 the phone's approval actions", () => {
  it("offers Allow and Deny for a thread's approval", () => {
    expect(approvalActions(base, true)).toEqual({ allow: true, deskOnly: false, deny: true });
  });

  it("offers no Allow for a pane's run, only Deny and Answer on your computer", () => {
    expect(approvalActions({ ...base, deskOnly: true }, true)).toEqual({
      allow: false,
      deskOnly: true,
      deny: true,
    });
  });

  it("offers nothing without the Approvals grant", () => {
    expect(approvalActions(base, false)).toMatchObject({ allow: false, deny: false });
  });
});
