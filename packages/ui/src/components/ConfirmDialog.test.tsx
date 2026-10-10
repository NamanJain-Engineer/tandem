import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Checkbox } from "./Choice";
import { ConfirmDialog } from "./Dialog";

function Harness({ onConfirm }: { onConfirm: (ticked: boolean) => void }) {
  const [ticked, setTicked] = useState(false);
  return (
    <ConfirmDialog
      open
      onOpenChange={() => {}}
      title="Delete all data?"
      body="Removes everything."
      confirmLabel="Delete data"
      danger
      requireText="delete"
      onConfirm={() => onConfirm(ticked)}
    >
      <Checkbox label="Also remove sign-ins" checked={ticked} onCheckedChange={setTicked} />
    </ConfirmDialog>
  );
}

describe("ConfirmDialog (UI-135)", () => {
  it("F-TL-05 extra controls sit between the body and the typed confirmation, and their state reaches onConfirm", () => {
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    const dialog = screen.getByRole("alertdialog");
    const box = screen.getByRole("checkbox", { name: "Also remove sign-ins" });
    const input = screen.getByLabelText('Type "delete" to confirm');
    expect(dialog.contains(box)).toBe(true);
    // The control comes before the typed confirmation.
    expect(box.compareDocumentPosition(input) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const confirm = screen.getByRole("button", { name: "Delete data" });
    expect((confirm as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(box);
    fireEvent.change(input, { target: { value: "delete" } });
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith(true);
  });
});
