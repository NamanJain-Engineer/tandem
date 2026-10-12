/** A dialog or a danger confirm (an `alertdialog`) is open. Panes leave
 * keyboard focus alone then: a dialog traps focus (UI_SPEC §2.10), so one
 * taken from it comes straight back with its field's text selected, and the
 * next key replaces what was typed (D-841). */
export function modalOpen(): boolean {
  return document.querySelector("[role=dialog],[role=alertdialog]") !== null;
}
