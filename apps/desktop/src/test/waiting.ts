/** Whether a control that waits (Allow until it has held still) is off for
 * now: it stays focusable, so it says so with `aria-disabled`. */
export function waiting(el: Element): boolean {
  return el.getAttribute("aria-disabled") === "true";
}
