import { inTauri, usingFakeBackend } from "./invoke";

/** Puts text on the system clipboard (Copy path, terminal copy). The Tauri
 * plugin works without page focus; the web API is the fallback in tests. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (inTauri() && !usingFakeBackend()) {
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
