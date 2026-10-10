import { $, browser } from "@wdio/globals";
import { crc32, deflateSync } from "node:zlib";
import { mod } from "./app";

/** Thread mode (Mod+3), with its composer showing. */
export async function threadMode(): Promise<void> {
  await browser.keys([mod(), "3"]);
  await composer().waitForDisplayed({ timeout: 10_000 });
}

export function composer() {
  return $('[data-testid="composer"] textarea[aria-label="Message"]');
}

/** Types into the composer and sends with Enter. */
export async function sendMessage(text: string): Promise<void> {
  const box = composer();
  await box.waitForDisplayed({ timeout: 10_000 });
  await box.click();
  await box.setValue(text);
  await browser.keys("Enter");
}

/** The transcript's assistant messages, in order. */
export async function assistantTexts(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="thread-assistant"]')).map((e) => e.innerText.trim()),
  );
}

export async function waitForAssistant(needle: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(async () => (await assistantTexts()).some((t) => t.includes(needle)), {
    timeout,
    timeoutMsg: `no reply containing ${JSON.stringify(needle)}: ${JSON.stringify(await assistantTexts())}`,
  });
}

/** Rail rows: title and group. */
export async function railRows(): Promise<Array<{ title: string; group: string }>> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="thread-row"]')).map((r) => ({
      title: r.getAttribute("title") ?? "",
      group: r.closest("section")?.getAttribute("aria-label") ?? "",
    })),
  );
}

export async function headerTitle(): Promise<string> {
  return String(await $('[data-testid="thread-title"]').getProperty("textContent")).trim();
}

/** A tiny RGBA PNG (`w`×`h`, one colour). */
export function png(w: number, h: number): Buffer {
  const chunk = (type: string, data: Buffer) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(w * 4, Buffer.from([0x2d, 0xd4, 0xbf, 0xff]))]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

/** The user messages shown, in order. */
export async function userTexts(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="thread-user"] p')).map((e) => e.innerText),
  );
}

/** The open thread's id (from the selected rail row). */
export async function openThreadId(): Promise<string> {
  return browser.execute(() => document.querySelector<HTMLElement>('[data-testid="thread-row"][aria-current="page"]')?.dataset.threadId ?? "");
}
