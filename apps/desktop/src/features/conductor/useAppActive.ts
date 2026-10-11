import { useEffect, useState } from "react";

function active(): boolean {
  if (typeof document === "undefined") return true;
  return document.visibilityState !== "hidden" && document.hasFocus();
}

/** The window is shown and the app has focus. The ring draws only then
 * (F-CN-04: rendering pauses when hidden or when the app is inactive). */
export function useAppActive(): boolean {
  const [on, setOn] = useState(active);
  useEffect(() => {
    const update = () => setOn(active());
    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
    };
  }, []);
  return on;
}
