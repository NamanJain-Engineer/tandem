import { useEffect } from "react";
import { useSetting } from "../../stores/settings";
import { COMMANDS, bindingFor, runCommand } from "../commands/commands";
import { matches, parseBinding, type Binding } from "../commands/keys";
import { useBoot } from "./useBoot";

/** Fields where typing wins over app shortcuts (xterm's textarea excepted). */
function isTextField(el: Element | null): boolean {
  if (!(el instanceof HTMLElement) || el.closest(".xterm")) return false;
  return el.isContentEditable || el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement;
}

/** WebView keys that would reload, print, save or open devtools (F-SH-09 AC3). */
export function isWebviewShortcut(e: Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "shiftKey">, mac: boolean): boolean {
  if (e.key === "F5" || e.key === "F3" || e.key === "F12") return true;
  if (!(mac ? e.metaKey : e.ctrlKey)) return false;
  if (e.shiftKey) return e.code === "KeyI" || e.code === "KeyJ" || e.code === "KeyR";
  return ["KeyR", "KeyP", "KeyF", "KeyS", "KeyU", "KeyG"].includes(e.code);
}

type Entry = { id: string; inText: boolean; normal: Binding | null; terminal: Binding | null };

/** The shortcut dispatcher (F-SH-09, UI_SPEC §5). Every binding comes from
 * the registry and runs the same handler as the palette. With a terminal
 * focused on Windows/Linux under "Terminal first", only the intercept list
 * is taken; everything else reaches the PTY. Nothing fires mid-composition. */
export function useShortcuts(): void {
  const platform = useBoot((s) => s.boot?.platform ?? "linux");
  const dev = useBoot((s) => s.boot?.dev ?? true);
  const priority = useSetting("term_shortcut_priority", "terminal");
  useEffect(() => {
    const mac = platform === "macos";
    const parse = (k: string | null) => (k ? parseBinding(k) : null);
    const entries: Entry[] = COMMANDS.map((spec) => ({
      id: spec.id,
      inText: spec.inText,
      normal: parse(bindingFor(spec, { mac, terminalFirst: false })),
      terminal: parse(bindingFor(spec, { mac, terminalFirst: true })),
    }));
    let composing = false;
    const onCompositionStart = () => {
      composing = true;
    };
    const onCompositionEnd = () => {
      composing = false;
    };
    const onKey = (e: KeyboardEvent) => {
      // The IME guard: composition keys belong to the IME (F-SH-09 AC5).
      if (composing || e.isComposing || e.keyCode === 229) return;
      const target = e.target instanceof Element ? e.target : null;
      const inTerminal = !!target?.closest(".xterm");
      const terminalFirst = !mac && inTerminal && priority === "terminal";
      const textFocus = isTextField(target) || !!document.querySelector("[role=dialog],[role=alertdialog]");
      for (const entry of entries) {
        const binding = terminalFirst ? entry.terminal : entry.normal;
        if (!binding || !matches(binding, e, mac)) continue;
        if (textFocus && !entry.inText) continue;
        if (runCommand(entry.id)) {
          e.preventDefault();
          e.stopPropagation();
          return;
        }
      }
      // Release builds never reload, print or open devtools. The key still
      // reaches a focused terminal (xterm ignores defaultPrevented).
      if (!dev && isWebviewShortcut(e, mac)) e.preventDefault();
    };
    window.addEventListener("compositionstart", onCompositionStart, true);
    window.addEventListener("compositionend", onCompositionEnd, true);
    window.addEventListener("keydown", onKey, { capture: true });
    return () => {
      window.removeEventListener("compositionstart", onCompositionStart, true);
      window.removeEventListener("compositionend", onCompositionEnd, true);
      window.removeEventListener("keydown", onKey, { capture: true });
    };
  }, [platform, priority, dev]);
}
