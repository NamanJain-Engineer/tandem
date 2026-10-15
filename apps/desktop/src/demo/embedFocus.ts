/** Inside the home page's frame (`?embed=1`), the demo takes no focus until
 * the visitor clicks or types in it. Its composer autofocuses, and focusing
 * inside a frame scrolls the page down to the frame and takes the keyboard
 * from it (D-1821). Once engaged, focus never scrolls the page. Returns a
 * function that puts `focus` back. */
export function holdFocusUntilEngaged(win: Window & typeof globalThis): () => void {
  const proto = win.HTMLElement.prototype;
  const original = proto.focus;
  let engaged = false;
  const engage = () => {
    engaged = true;
  };
  win.addEventListener("pointerdown", engage, { capture: true });
  win.addEventListener("keydown", engage, { capture: true });
  proto.focus = function focus(this: HTMLElement, options?: FocusOptions) {
    if (engaged) original.call(this, { ...options, preventScroll: true });
  };
  return () => {
    proto.focus = original;
    win.removeEventListener("pointerdown", engage, { capture: true });
    win.removeEventListener("keydown", engage, { capture: true });
  };
}
