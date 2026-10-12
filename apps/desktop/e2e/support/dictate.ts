import { browser } from "@wdio/globals";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { home, invoke } from "./app";

/** The committed WAV fixtures (scripts/gen-voice-fixtures.sh). */
export function fixture(name: string): string {
  return join(process.cwd(), "../../crates/tandem-voice/tests/fixtures", name);
}

type ModelsView = {
  active: string | null;
  pending: string | null;
  models: Array<{ id: string; state: string; error: string | null; available: boolean }>;
  usedBytes: number;
};

export const BASE_EN = "whisper-base.en";

export async function models(): Promise<ModelsView> {
  return invoke<ModelsView>("dictate_models");
}

/** What the local model server does next (scripts/serve-models.ts --control). */
export function modelServer(mode: "normal" | "slow" | { corruptByte: number }): void {
  const value = mode === "normal" ? "" : mode === "slow" ? "slow" : String(mode.corruptByte);
  writeFileSync(process.env.TANDEM_MODELS_CONTROL ?? join(home(), "models-control.txt"), value);
}

/** Downloads whisper base.en from the local server (if needed) and waits
 * until it's the active model. */
export async function ensureModel(id = BASE_EN): Promise<void> {
  modelServer("normal");
  let view = await models();
  if (view.active === id) return;
  const row = view.models.find((m) => m.id === id);
  if (row?.state === "downloaded") await invoke("dictate_model_select", { modelId: id });
  else await invoke("dictate_model_download", { modelId: id });
  await browser.waitUntil(
    async () => {
      view = await models();
      return view.active === id;
    },
    { timeout: 120_000, interval: 300, timeoutMsg: `${id} never became active: ${JSON.stringify(view)}` },
  );
}

export async function key(which: "hold" | "toggle" | "esc", phase: "down" | "up"): Promise<boolean> {
  return invoke<boolean>("test_dictate_key", { which, phase });
}

export async function mic(
  source: { source: "wav"; path: string } | { source: "silence" } | { source: "zeros" },
): Promise<void> {
  await invoke("test_dictate_mic", { source });
}

export type PlatformView = { calls: string[]; pasted: string[]; clipboard: string | null };

export const TEXT_EDITOR = {
  appId: "org.gnome.TextEditor",
  appName: "Text Editor",
  window: 4242,
  element: 0,
  terminal: false,
  elevated: false,
  own: false,
};

export async function platform(args: Record<string, unknown> = {}): Promise<PlatformView> {
  return invoke<PlatformView>("test_dictate_platform", args);
}

/** A clean insertion backend over a text editor, with something on the clipboard. */
export async function resetPlatform(target: Record<string, unknown> | null = TEXT_EDITOR): Promise<void> {
  await platform({
    clear: true,
    failFocus: false,
    failPaste: false,
    clipboard: "previous clipboard",
    ...(target ? { target } : { noTarget: true }),
  });
}

export async function waitPasted(timeout = 60_000): Promise<PlatformView> {
  let view: PlatformView = { calls: [], pasted: [], clipboard: null };
  await browser.waitUntil(
    async () => {
      view = await platform();
      return view.pasted.length > 0;
    },
    { timeout, interval: 200, timeoutMsg: "nothing was pasted" },
  );
  return view;
}

/** Hold the shortcut for `ms`, then release. */
export async function hold(ms: number): Promise<void> {
  await key("hold", "down");
  await browser.pause(ms);
  await key("hold", "up");
}

export async function setSetting(key: string, value: unknown): Promise<void> {
  await invoke("settings_set", { key, value });
}

export async function widget(): Promise<{ exists: boolean; visible: boolean }> {
  return invoke<{ exists: boolean; visible: boolean }>("test_dictate_widget");
}

export function words(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9\s]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}
