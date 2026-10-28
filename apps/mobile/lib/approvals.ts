// F-RM-05 / F-TL-12: what the Approvals tab offers for one approval. Kept
// out of the screen so it runs in tests without React Native.
import type { PhoneApproval } from "@tandem/protocol";

export type ApprovalActions = {
  /** Allow is offered (a pane's `vibezeo run` is answered on the computer). */
  allow: boolean;
  /** "Answer on your computer" shows in Allow's place. */
  deskOnly: boolean;
  deny: boolean;
};

export function approvalActions(a: PhoneApproval, granted: boolean): ApprovalActions {
  return { allow: granted && !a.deskOnly, deskOnly: a.deskOnly, deny: granted };
}
