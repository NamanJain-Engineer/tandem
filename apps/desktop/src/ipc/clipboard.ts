import { inTauri, usingFakeBackend } from "./invoke";

const native = () => inTauri() && !usingFakeBackend();

/** Puts text on the system clipboard (Copy path, terminal copy). The Tauri
 * plugin works without page focus; the web API is the fallback in tests. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (native()) {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    } else {
      await navigator.clipboard.writeText(text);
    }
    return true;
  } catch {
    return false;
  }
}

/** Text on the system clipboard, or "" (an image, or nothing). */
export async function readText(): Promise<string> {
  try {
    if (native()) {
      const { readText: read } = await import("@tauri-apps/plugin-clipboard-manager");
      return (await read()) ?? "";
    }
    return await navigator.clipboard.readText();
  } catch {
    return "";
  }
}
