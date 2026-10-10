import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Mail, ThreadEvent, ThreadEventEnvelope } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { MailRowView } from "./MailRow";
import { emptyThread, reduce } from "./reducer";
import { toRows } from "./rows";

function env(turnId: string, seq: number, event: ThreadEvent): ThreadEventEnvelope {
  return { threadId: "thr_1", turnId, seq, ts: 0, event };
}

const row = (inbound: boolean): ThreadEvent => ({
  type: "mail_row",
  mailId: "ml_1",
  from: "Ada",
  to: "Bea",
  preview: "Check CI on main",
  inbound,
});

const MAIL: Mail = {
  id: "ml_1",
  fromTeammateId: "tm_a",
  from: "Ada",
  toTeammateId: "tm_b",
  to: "Bea",
  fromThreadId: "th_a",
  body: "Check CI on main\nIt's red since this morning.",
  conversationRef: "th_a",
  state: "failed",
  reason: "wake_failed",
  deliveredThreadId: null,
  createdAt: 0,
  deliveredAt: null,
} as unknown as Mail;

describe("F-AG-12 mail in transcripts", () => {
  beforeEach(() => resetStores());

  it("F-AG-12.1 a sender's row is a turn of its own; a delivery shows its row in place of the message", () => {
    const sender = [
      env("t1", 0, row(false)),
      env("t1", 1, { type: "turn_end", reason: "completed", blockedActions: 0 }),
    ].reduce(reduce, emptyThread);
    expect(toRows(sender)).toEqual([
      { kind: "mail", key: "t1:0", mailId: "ml_1", from: "Ada", to: "Bea", preview: "Check CI on main" },
    ]);
    const recipient = [
      env("t2", 0, { type: "user", text: "From Ada:\nCheck CI on main", attachments: [] }),
      env("t2", 1, row(true)),
      env("t2", 2, { type: "assistant", markdown: "On it." }),
    ].reduce(reduce, emptyThread);
    const rows = toRows(recipient);
    expect(rows.map((r) => r.kind)).toEqual(["mail", "assistant", "working"]);
    expect(rows.some((r) => r.kind === "user")).toBe(false);
  });

  it("F-AG-12 Open shows the full message and its state; Retry puts a failed mail back", async () => {
    let mail = MAIL;
    const calls = fakeBackend({
      mail_get: () => mail,
      mail_retry: () => {
        mail = { ...mail, state: "delivered", reason: null } as Mail;
        return null;
      },
    });
    render(
      <UiProvider>
        <MailRowView mailId="ml_1" from="Ada" to="Bea" preview="Check CI on main" />
      </UiProvider>,
    );
    expect(screen.getByTestId("mail-row").textContent).toContain("Ada → Bea: “Check CI on main”");
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect((await screen.findByTestId("mail-body")).textContent).toBe(MAIL.body);
    expect(screen.getByTestId("mail-state").textContent).toBe("Failed: Bea couldn't start a chat");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByTestId("mail-state").textContent).toBe("Delivered"));
    expect(calls).toContainEqual({ cmd: "mail_retry", args: { mailId: "ml_1" } });
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });
});
