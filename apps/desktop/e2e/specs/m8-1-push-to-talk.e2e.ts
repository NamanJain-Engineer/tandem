import { browser } from "@wdio/globals";
import { freshOffline, invoke } from "../support/app";
import {
  TEXT_EDITOR,
  ensureModel,
  fixture,
  hold,
  key,
  mic,
  platform,
  resetPlatform,
  setSetting,
  waitPasted,
  words,
} from "../support/dictate";

// E2E-M8-1..3 (F-DI-01, F-DI-02, F-DI-05, F-DI-06): the fake hotkey, the
// fake microphone playing a WAV and the recording insertion backend, with
// whisper base.en served by scripts/serve-models.ts.

type Stats = { words: number; takes: number };
const stats = () => invoke<Stats>("dictate_stats");

describe("E2E-M8-1 Push-to-talk", () => {
  before(async () => {
    await freshOffline();
    await setSetting("dictate_enabled", true);
    await setSetting("dictate_output", "paste");
    await ensureModel();
  });

  it("E2E-M8-1 hold → WAV → release pastes 'hello world' and restores the clipboard", async () => {
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    const view = await waitPasted();
    expect(words(view.pasted[0] ?? "")).toContain("hello world");
    expect(view.calls).toContain("restore_focus:4242");
    expect(view.calls).toContain("paste:text");
    await browser.waitUntil(async () => (await platform()).clipboard === "previous clipboard", {
      timeout: 5_000,
      timeoutMsg: "the previous clipboard wasn't restored",
    });
  });
});

describe("E2E-M8-2 Taps and cancel", () => {
  it("E2E-M8-2 a 100 ms tap records nothing", async () => {
    await resetPlatform();
    const before = await stats();
    await hold(100);
    await browser.pause(1_500);
    expect((await platform()).pasted).toEqual([]);
    expect((await stats()).takes).toBe(before.takes);
  });

  it("E2E-M8-2 Esc while recording: no transcription and no insertion", async () => {
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    const before = await stats();
    await key("hold", "down");
    await browser.pause(1_000);
    await key("esc", "down");
    await key("esc", "up");
    await key("hold", "up");
    await browser.pause(3_000);
    expect((await platform()).pasted).toEqual([]);
    expect((await stats()).takes).toBe(before.takes);
  });

  it("E2E-M8-2 a toggle binding of a bare modifier is refused", async () => {
    let message = "";
    try {
      await invoke("dictate_set_binding", { binding: "Ctrl", mode: "toggle" });
    } catch (e) {
      message = (e as { message?: string }).message ?? String(e);
    }
    expect(message).not.toBe("");
  });
});

describe("E2E-M8-3 Guards", () => {
  it("E2E-M8-3 2 s of silence says No speech detected and inserts nothing", async () => {
    await resetPlatform();
    await mic({ source: "wav", path: fixture("silence_2s.wav") });
    await hold(2_200);
    await browser.waitUntil(
      async () => {
        const text = await browser.execute(() => document.body.innerText);
        return text.includes("No speech detected");
      },
      { timeout: 30_000, timeoutMsg: "no 'No speech detected' toast" },
    );
    expect((await platform()).pasted).toEqual([]);
  });

  it("E2E-M8-3 a focus-restore failure leaves the text on the clipboard", async () => {
    await resetPlatform();
    await platform({ failFocus: true });
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    await browser.waitUntil(async () => words((await platform()).clipboard ?? "").includes("hello world"), {
      timeout: 60_000,
      timeoutMsg: "the text never reached the clipboard",
    });
    expect((await platform()).pasted).toEqual([]);
    await platform({ failFocus: false });
  });

  it("E2E-M8-3 control characters never reach a terminal, and no CR is sent", async () => {
    // A correction that writes an escape sequence and a carriage return.
    const rule = await invoke<{ id: string }>("dictionary_save", {
      rule: {
        id: "",
        kind: "correction",
        replacement: "\u001b[31mhello\r",
        phrases: ["hello"],
        matchCase: false,
        protect: false,
        spacing: "exact",
        enabled: true,
        lang: "*",
        appProfiles: [],
        updatedAt: 0,
        deletedAt: null,
      },
    });
    try {
      await resetPlatform({ ...TEXT_EDITOR, appId: "org.gnome.Terminal", appName: "Terminal", terminal: true });
      await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
      await hold(2_500);
      const view = await waitPasted();
      const pasted = view.pasted[0] ?? "";
      expect(view.calls).toContain("paste:terminal");
      expect(pasted).not.toContain("\u001b");
      expect(pasted).not.toContain("\r");
      expect(pasted).not.toContain("\n");
    } finally {
      await invoke("dictionary_delete", { ids: [rule.id] });
    }
  });
});
