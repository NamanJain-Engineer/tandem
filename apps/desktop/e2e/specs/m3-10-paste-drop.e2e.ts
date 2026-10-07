import { $, browser } from "@wdio/globals";
import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { crc32, deflateSync } from "node:zlib";
import { addFolder, freshOffline, invoke, paneIds, paneText, rawRead, sendToPane, split, waitForPaneText } from "../support/app";

/** A tiny RGBA PNG (`w`×`h`, one colour). */
function png(w: number, h: number): Buffer {
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

async function center(pane: string): Promise<{ x: number; y: number }> {
  return (await browser.execute((id: string) => {
    const r = document.querySelector(`[data-testid="terminal"][data-pane-id="${id}"]`)?.getBoundingClientRect();
    return r ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : { x: -1, y: -1 };
  }, pane)) as { x: number; y: number };
}

async function drop(paths: string[], at: { x: number; y: number }): Promise<void> {
  await browser.executeAsync(
    (p: string[], x: number, y: number, done: () => void) => {
      const f = (window as unknown as { __tandemDrop: (p: string[], x: number, y: number) => Promise<void> }).__tandemDrop;
      void f(p, x, y).then(done);
    },
    paths,
    at.x,
    at.y,
  );
}

// E2E-M3-10 Paste and drop (F-CD-05, F-CD-06).
describe("E2E-M3-10 paste and drop", () => {
  const root = `/tmp/tandem-e2e-m3-10-${Date.now()}`;
  let shell = "";

  before(async () => {
    mkdirSync(root, { recursive: true });
    await freshOffline();
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    shell = (await paneIds())[0] ?? "";
    await waitForPaneText(shell, "#");
  });

  it("F-CD-05.2 pasting a\\nb into a bracketed-paste program sends ESC[200~a\\nb ESC[201~ after the confirm", async () => {
    await invoke("plugin:clipboard-manager|write_text", { text: "a\nb" });
    const read = await rawRead(shell, 15, 15, { bracketed: true });
    await $(`[data-testid="terminal"][data-pane-id="${shell}"]`).click();
    await browser.keys(["Control", "Shift", "v"]);
    const dialog = $('[role="dialog"]');
    await dialog.waitForDisplayed({ timeout: 5_000 });
    expect(await dialog.getText()).toContain("Paste 2 lines?");
    await dialog.$("button=Paste").click();
    expect(await read()).toBe("1b 5b 32 30 30 7e 61 0a 62 1b 5b 32 30 31 7e");
  });

  it("F-CD-05.3 an image pasted into an agent pane is staged as a PNG and its quoted path typed", async () => {
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    const file = `${root}/clip.png`;
    writeFileSync(file, png(3, 2));
    // xclip keeps serving the clipboard in the background until replaced.
    const owner = spawn("xclip", ["-selection", "clipboard", "-t", "image/png", "-i", file], { stdio: "ignore", detached: true });
    owner.unref();
    await browser.pause(500);
    await $(`[data-testid="terminal"][data-pane-id="${agent}"]`).click();
    await browser.keys(["Control", "Shift", "v"]);
    await browser.pause(1_000);
    await sendToPane(agent, "\r");
    await browser.waitUntil(async () => /you said: .*'([^']+\.png)'/.test(await paneText(agent)), {
      timeout: 10_000,
      timeoutMsg: `no staged path typed: ${await paneText(agent)}`,
    });
    const staged = /you said: .*'([^']+\.png)'/.exec(await paneText(agent))?.[1] ?? "";
    const bytes = readFileSync(staged);
    expect(bytes.subarray(1, 4).toString()).toBe("PNG");
    expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)]).toEqual([3, 2]);
    try {
      execFileSync("pkill", ["-f", `xclip -selection clipboard -t image/png -i ${file}`]);
    } catch {
      // already gone
    }
    await $(`[data-testid="pane"][data-pane-id="${agent}"] button[aria-label^="Close "]`).click();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 10_000 });
  });

  it("F-CD-06.1 a drop lands in the pane under the pointer (3rd of 4) and is quoted", async () => {
    for (let i = 0; i < 3; i++) await split(i % 2 ? "down" : "right");
    const ids = await paneIds();
    expect(ids).toHaveLength(4);
    const target = ids[2] ?? "";
    const odd = `${root}/a b'c`;
    mkdirSync(odd, { recursive: true });
    await drop([odd], await center(target));
    // The drop is typed onto the shell's command line (zsh breaks it at the pane's width).
    await waitForPaneText(target, `'${root}/a b'\\''c'`, 15_000, { commandLine: true });
    for (const other of ids.filter((id) => id !== target)) expect(await paneText(other)).not.toContain("a b'");
    await sendToPane(target, "\x15");
  });

  it("F-CD-06.3 a file name with a newline is refused with a toast", async () => {
    const target = (await paneIds())[0] ?? "";
    await drop([`${root}/bad\nname`], await center(target));
    await browser.waitUntil(async () => (await browser.execute(() => document.body.innerText)).includes("This file name can't be inserted safely"), {
      timeout: 5_000,
      timeoutMsg: "no refusal toast",
    });
    expect(await paneText(target)).not.toContain("bad");
  });
});
