import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { DictateStatus, ModelsView, Rule, TakePage } from "@tandem/protocol";
import { Composer, UiProvider, type ComposerHandle } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDictate, watchDictate } from "../../stores/dictate";
import { BOOT, fakeBackend, resetStores } from "../../test/fakeBackend";
import { useBoot } from "../shell/useBoot";
import { bindingFrom, keyName, presets } from "./binding";
import { DictateExtras, DictateHomeCard, MicSelect, ShortcutField } from "./DictatePage";
import { StatusBar } from "../shell/StatusBar";
import { EngineControl } from "./EngineControl";
import { AutoEnhanceControl, FormattingControl } from "./EnhanceControls";
import { useSettings } from "../../stores/settings";
import { useSession } from "../../stores/session";

/** An account that carries Enhance (hosted, D-2903). */
function withEnhance() {
  const e = useSession.getState().entitlement!;
  return { ...e, limits: { ...e.limits, enhance: true } };
}
import { Permissions, ShortcutChoice, TryIt } from "./DictateSetup";
import { GlobeKey } from "./GlobeKey";
import { DictionarySheet, emptyRule } from "./DictionarySheet";
import { HistorySheet } from "./HistorySheet";
import { ModelManager } from "./ModelManager";
import { mainWindowShows, toastText } from "./toasts";
import { useComposerDictation } from "./useComposerDictation";
import { DictateWidget, clock } from "./Widget";

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

const STATUS: DictateStatus = {
  enabled: true,
  hotkey: { kind: "active" },
  verified: false,
  hold: { binding: "Ctrl+Alt", label: "Ctrl+Alt" },
  toggle: null,
  latch: true,
  session: "x11",
  deepLinkCommand: null,
  mic: null,
  engine: "local",
  model: { id: "whisper-base.en", name: "Whisper Base (English)" },
  modelStage: "ready",
  fallback: null,
  state: "idle",
  missingTools: null,
  canPauseMedia: true,
  restoresClipboard: true,
  theme: "dark",
  widgetStyle: "logo_text",
  lockPosition: false,
  quickActions: false,
  enhanceDisclosed: false,
  fnFallback: false,
};

const wrap = (ui: React.ReactNode) => render(<UiProvider>{ui}</UiProvider>);

describe("Dictate bindings (F-DI-01)", () => {
  it("F-DI-01 recorded keys become binding strings, modifier-only included", () => {
    expect(bindingFrom({ code: "", ctrlKey: true, altKey: true, shiftKey: false, metaKey: false })).toBe(
      "Ctrl+Alt",
    );
    expect(bindingFrom({ code: "KeyD", ctrlKey: true, altKey: false, shiftKey: true, metaKey: false })).toBe(
      "Ctrl+Shift+D",
    );
    expect(
      bindingFrom({ code: "Mouse4", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false }),
    ).toBe("Mouse4");
    expect(keyName("IntlBackslash")).toBeNull();
    expect(presets("macos").recommended).toBe("Fn");
    expect(presets("windows").recommended).toBe("Ctrl+Meta");
    expect(presets("linux").recommended).toBe("Ctrl+Alt");
  });

  it("F-DI-01.4 a modifier-only toggle shows the core's validation error", async () => {
    fakeBackend({
      dictate_set_binding: () => {
        throw { code: "invalid", message: "Toggle needs a key that isn't only modifiers." };
      },
    });
    useDictate.setState({ status: STATUS });
    wrap(<ShortcutField mode="toggle" label="Toggle (optional)" />);
    const rec = screen.getByRole("button", { name: "Toggle (optional)" });
    fireEvent.click(rec);
    fireEvent.keyDown(rec, { code: "ControlLeft", key: "Control", ctrlKey: true });
    fireEvent.keyDown(rec, { code: "AltLeft", key: "Alt", ctrlKey: true, altKey: true });
    fireEvent.keyUp(rec, { code: "AltLeft", key: "Alt" });
    await waitFor(() =>
      expect(screen.getByTestId("dictate-shortcut-error-toggle").textContent).toBe(
        "Toggle needs a key that isn't only modifiers.",
      ),
    );
  });
});

describe("Dictate widget (F-DI-07, UI-60)", () => {
  it("F-DI-07 listening shows the timer and ×, hands-free names the key, insertion counts words", async () => {
    fakeBackend({ dictate_status: () => STATUS });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() =>
      emitLocal("dictate:state", {
        state: "listening",
        takeId: "take_1",
        handsFree: true,
        composer: null,
        words: null,
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("widget-text").textContent).toBe("Hands-free · press Ctrl+Alt to stop"),
    );
    expect(screen.getByTestId("widget-timer").textContent).toBe("0:00");
    expect(screen.getByRole("button", { name: "Discard this take" })).toBeTruthy();
    act(() =>
      emitLocal("dictate:state", {
        state: "inserted",
        takeId: "take_1",
        handsFree: false,
        composer: null,
        words: 42,
      }),
    );
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Inserted · 42 words"));
    expect(clock(65_000)).toBe("1:05");
  });

  it("F-DI-07 a widget opened mid-take shows the take (D-519)", async () => {
    fakeBackend({ dictate_status: () => ({ ...STATUS, state: "recording" }) });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Listening"));
    expect(screen.getByTestId("widget-timer").textContent).toBe("0:00");
  });

  it("F-DI-07.2 an error toast renders with its full diagnostic in the tooltip", async () => {
    fakeBackend({ dictate_status: () => STATUS });
    wrap(<DictateWidget />);
    act(() =>
      emitLocal("dictate:toast", {
        code: "mic_silent",
        error: true,
        app: null,
        model: null,
        previous: null,
        detail:
          "No samples arrived within 1 s, after rebuilding the stream and trying the backup microphone.",
        message: null,
        actions: [],
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("widget-text").textContent).toBe("Your microphone isn't delivering audio"),
    );
    const pill = screen.getByTestId("dictate-widget");
    expect(pill.getAttribute("title")).toBe(
      "No samples arrived within 1 s, after rebuilding the stream and trying the backup microphone.",
    );
    expect(pill.getAttribute("data-state")).toBe("error");
  });

  it("F-DI-07 toasts read from the catalog; composer takes toast in the main window", () => {
    const t = {
      code: "elevated" as const,
      error: false,
      app: "Registry Editor",
      model: null,
      previous: null,
      detail: "",
      message: null,
      actions: [],
    };
    expect(toastText(t)).toBe(
      "Registry Editor is running as administrator, so Vibezeo can't type into it. Copied instead.",
    );
    expect(mainWindowShows({ ...t, code: "no_speech" }, true, false)).toBe(true);
    expect(mainWindowShows({ ...t, code: "no_speech" }, false, false)).toBe(false);
    expect(mainWindowShows({ ...t, code: "model_fallback" }, false, false)).toBe(true);
  });
});

function DictatedComposer({ handle }: { handle: React.RefObject<ComposerHandle | null> }) {
  const [value, setValue] = useStateValue("fix the  test");
  const { dictation, onFocusChange } = useComposerDictation("thread:t1", handle);
  return (
    <Composer
      ref={handle}
      value={value}
      onChange={setValue}
      onSend={() => undefined}
      placeholder="Message"
      dictation={dictation}
      onFocusChange={onFocusChange}
    />
  );
}

import { useState } from "react";
function useStateValue(initial: string) {
  return useState(initial);
}

describe("Composer dictation (F-DI-12, UI-68)", () => {
  it("F-DI-12.1 dictated text goes in at the caret, and the clipboard is never used", async () => {
    const calls = fakeBackend();
    const off = await watchDictate();
    const handle = createRef<ComposerHandle>();
    wrap(<DictatedComposer handle={handle} />);
    const area = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
    area.focus();
    area.setSelectionRange(8, 8);
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === "dictate_composer_focus" && c.args?.targetId === "thread:t1")).toBe(
        true,
      ),
    );
    act(() => emitLocal("dictate:insert", { targetId: "thread:other", text: "nope" }));
    act(() => emitLocal("dictate:insert", { targetId: "thread:t1", text: "failing" }));
    await waitFor(() => expect(area.value).toBe("fix the failing test"));
    expect(calls.some((c) => c.cmd.includes("clipboard"))).toBe(false);
    off();
  });

  it("F-DI-12 a click toggles hands-free, a press-and-hold stops on release", async () => {
    const calls = fakeBackend();
    const off = await watchDictate();
    const handle = createRef<ComposerHandle>();
    wrap(<DictatedComposer handle={handle} />);
    const mic = screen.getByTestId("composer-mic");
    fireEvent.pointerDown(mic, { button: 0 });
    fireEvent.pointerUp(mic, { button: 0 });
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "dictate_start_take")[0]?.args).toEqual({
        origin: { type: "composer", targetId: "thread:t1" },
      }),
    );
    expect(calls.some((c) => c.cmd === "dictate_stop_take")).toBe(false);
    act(() =>
      emitLocal("dictate:state", {
        state: "listening",
        takeId: "take_1",
        handsFree: true,
        composer: "thread:t1",
        words: null,
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("composer-mic").getAttribute("data-state")).toBe("listening"),
    );
    expect((screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement).placeholder).toBe(
      "Listening…",
    );
    fireEvent.pointerDown(screen.getByTestId("composer-mic"), { button: 0 });
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_stop_take")).toBe(true));
    off();
  });
});

describe("Dictate setup (F-DI-11)", () => {
  it("F-DI-11.2 Try it passes only after the press and the release", async () => {
    const calls = fakeBackend({ dictate_status: () => ({ ...STATUS, verified: true }) });
    useDictate.setState({ status: STATUS });
    wrap(<TryIt />);
    fireEvent.click(screen.getByTestId("dictate-verify"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_verify_start")).toBe(true));
    act(() => emitLocal("dictate:key", { phase: "down", native: true }));
    await waitFor(() => expect(screen.getByTestId("dictate-try-it").getAttribute("data-phase")).toBe("down"));
    expect(screen.queryByTestId("dictate-key-detected")).toBeNull();
    act(() => emitLocal("dictate:key", { phase: "up", native: true }));
    expect(screen.queryByTestId("dictate-key-detected")).toBeNull();
    act(() => emitLocal("dictate:verified", null));
    await waitFor(() => expect(screen.getByTestId("dictate-key-detected")).toBeTruthy());
  });
});

describe("Microphone choice (F-DI-03)", () => {
  it("D-518 an unset microphone shows its label in the trigger, not a blank", async () => {
    fakeBackend({
      dictate_devices: () => ({
        devices: [{ id: "mic1", name: "Desk Mic", isDefault: true, bluetooth: false, loopback: false }],
        chosen: null,
      }),
    });
    wrap(
      <>
        <MicSelect k="dictate_mic" label="Microphone" none="None" />
        <MicSelect k="dictate_backup_mic" label="Backup microphone" none="None" />
      </>,
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Microphone" }).textContent).toContain(
        "System default (Desk Mic)",
      ),
    );
    expect(screen.getByRole("combobox", { name: "Backup microphone" }).textContent).toContain("None");
  });
});

const page = (takes: TakePage["takes"]): TakePage => ({ takes, next: null });

describe("History and stats (F-DI-08, UI-65)", () => {
  it("F-DI-08.1 search asks the core, export writes the CSV, clear confirms", async () => {
    const take = {
      id: "take_1",
      createdAt: 0,
      text: "refactor the auth guard",
      engine: "local",
      model: "whisper-base.en",
      wordCount: 4,
      durationMs: 3000,
      appId: null,
    };
    const calls = fakeBackend({
      dictate_history: (a) => page(a?.query === "zzz" ? [] : [take]),
      dictate_stats: () => ({ words: 4, speakingMs: 3000, takes: 1, wpm: 80, savedMs: 3000 }),
      dictate_save_file: () => "/tmp/history.csv",
    });
    wrap(<HistorySheet open onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("take-text").textContent).toBe("refactor the auth guard"));
    fireEvent.change(screen.getByTestId("history-search"), { target: { value: "zzz" } });
    await waitFor(() => expect(screen.getByText("No takes match")).toBeTruthy());
    fireEvent.click(screen.getByTestId("history-export"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictate_history_export")?.args).toEqual({
        path: "/tmp/history.csv",
      }),
    );
    fireEvent.click(screen.getByTestId("history-clear"));
    fireEvent.click(await screen.findByRole("button", { name: "Clear history" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_history_clear")).toBe(true));
  });

  it("F-DI-08 the home card lists the last 5 takes, and one opens History", async () => {
    const takes = Array.from({ length: 5 }, (_, i) => ({
      id: `take_${i}`,
      createdAt: 0,
      text: `take number ${i}`,
      engine: "local",
      model: "whisper-base.en",
      wordCount: 3,
      durationMs: 1000,
      appId: null,
    }));
    const calls = fakeBackend({
      dictate_history: () => page(takes),
      dictate_stats: () => ({ words: 15, speakingMs: 5000, takes: 5, wpm: 90, savedMs: 4000 }),
    });
    useDictate.setState({ status: { ...STATUS, verified: true } });
    let opened = 0;
    wrap(<DictateHomeCard onTry={() => undefined} onHistory={() => (opened += 1)} />);
    await waitFor(() => expect(screen.getAllByTestId("dictate-recent-take")).toHaveLength(5));
    expect(calls.find((c) => c.cmd === "dictate_history")?.args).toEqual({
      query: null,
      before: null,
      limit: 5,
    });
    expect(screen.getAllByTestId("dictate-recent-take")[0]?.textContent).toContain("take number 0");
    fireEvent.click(screen.getAllByTestId("dictate-recent-take")[2] as HTMLElement);
    expect(opened).toBe(1);
  });

  it("F-DI-08 with no takes yet, the home card lists none", async () => {
    fakeBackend({ dictate_history: () => page([]) });
    useDictate.setState({ status: STATUS });
    wrap(<DictateHomeCard onTry={() => undefined} onHistory={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("dictate-home")).toBeTruthy());
    expect(screen.queryByTestId("dictate-recent")).toBeNull();
  });

  it("F-DI-08 history says which engine heard each take", async () => {
    const local = {
      id: "take_1",
      createdAt: 0,
      text: "local words",
      engine: "local",
      model: "whisper-base.en",
      wordCount: 2,
      durationMs: 1000,
      appId: null,
    };
    const cloud = { ...local, id: "take_2", text: "cloud words", engine: "cloud", model: "cloud" };
    fakeBackend({ dictate_history: () => page([local, cloud]) });
    wrap(<HistorySheet open onClose={() => undefined} />);
    await waitFor(() => expect(screen.getAllByTestId("take-text")).toHaveLength(2));
    const meta = (id: string) => document.querySelector(`[data-take="${id}"] span`)?.textContent ?? "";
    expect(meta("take_1")).toContain("Local · whisper-base.en");
    expect(meta("take_2")).toContain("Cloud");
    expect(meta("take_2")).not.toContain("Local");
  });

  it("F-DI-08 an empty history says how to start", async () => {
    fakeBackend({ dictate_history: () => page([]) });
    useDictate.setState({ status: STATUS });
    wrap(<HistorySheet open onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText("Nothing dictated yet")).toBeTruthy());
    expect(screen.getByText("Hold Ctrl+Alt and speak. Your takes appear here.")).toBeTruthy();
  });
});

describe("Dictionary (F-DI-09, UI-64)", () => {
  it("F-DI-09 adding a correction saves it, and the test box shows the result", async () => {
    const calls = fakeBackend({
      dictionary_list: () => [],
      dictionary_suggestions: () => [],
      dictionary_test: () => "use Tandem, https://tandem.dev",
      dictionary_save: (a) => a?.rule,
    });
    wrap(<DictionarySheet open onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByText("No words yet")).toBeTruthy());
    fireEvent.mouseDown(screen.getByRole("tab", { name: /Corrections/ }), { button: 0 });
    fireEvent.click(await screen.findByTestId("rules-add-correction"));
    fireEvent.change(
      screen.getByTestId("rule-phrases").querySelector("textarea") ?? screen.getByTestId("rule-phrases"),
      { target: { value: "tandem" } },
    );
    fireEvent.change(
      screen.getByTestId("rule-replacement").querySelector("input") ?? screen.getByTestId("rule-replacement"),
      { target: { value: "Tandem" } },
    );
    fireEvent.click(screen.getByTestId("rule-save"));
    await waitFor(() => {
      const saved = calls.find((c) => c.cmd === "dictionary_save")?.args?.rule as
        { kind: string; phrases: string[]; replacement: string } | undefined;
      expect(saved?.kind).toBe("correction");
      expect(saved?.phrases).toEqual(["tandem"]);
      expect(saved?.replacement).toBe("Tandem");
    });
    const box = screen.getByTestId("dictionary-test-input");
    fireEvent.change(box.querySelector("input") ?? box, {
      target: { value: "use tandem, https://tandem.dev" },
    });
    await waitFor(() =>
      expect(screen.getByTestId("dictionary-test-output").textContent).toBe("use Tandem, https://tandem.dev"),
    );
  });

  const proSync = () =>
    act(() => {
      const e = useSession.getState().entitlement!;
      useSession.setState({ entitlement: { ...e, limits: { ...e.limits, dictionarySync: true } } });
    });
  const word = (id: string, to: string): Rule => ({ ...emptyRule("correction"), id, phrases: [id], replacement: to });

  it("F-DI-09 sync is hosted: with no account it says it comes with Vibezeo accounts and nothing is sent", async () => {
    const calls = fakeBackend({ dictionary_list: () => [], dictionary_suggestions: () => [] });
    wrap(<DictionarySheet open onClose={() => undefined} />);
    const label = (await screen.findByTestId("dictionary-sync-pro")).textContent;
    expect(label).toContain("Comes with Vibezeo accounts");
    expect(label).toContain("It uses Vibezeo's servers and switches on when Vibezeo accounts open.");
    expect(label).toContain("Tell me when accounts open");
    expect(calls.some((c) => c.cmd.startsWith("dictionary_sync"))).toBe(false);
  });

  it("F-DI-09 sync stays off until turned on, then a conflict keeps the user's pick", async () => {
    const conflict = { id: "tauri", local: word("tauri", "Tauri."), remote: word("tauri", "TAURI") };
    const calls = fakeBackend({
      dictionary_list: () => [],
      dictionary_suggestions: () => [],
      dictionary_sync: () => ({ pushed: 1, pulled: 0, conflicts: [conflict], syncedAt: 1_700_000_000_000 }),
      dictionary_sync_resolve: () => conflict.local,
    });
    proSync();
    wrap(<DictionarySheet open onClose={() => undefined} />);
    const toggle = await screen.findByRole("switch", { name: "Sync dictionary" });
    expect(calls.some((c) => c.cmd === "dictionary_sync")).toBe(false);
    fireEvent.click(toggle);
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictionary_sync")).toBe(true));
    expect(calls.find((c) => c.cmd === "settings_set")?.args).toMatchObject({
      key: "dictate_dictionary_sync",
      value: true,
    });
    expect(await screen.findByText("tauri → Tauri.")).toBeTruthy();
    expect(screen.getByText("tauri → TAURI")).toBeTruthy();
    fireEvent.click(screen.getByTestId("dictionary-sync-keep-mine"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictionary_sync_resolve")?.args).toEqual({ id: "tauri", keep: "mine" }),
    );
    await waitFor(() => expect(screen.queryByTestId("dictionary-sync-conflict")).toBeNull());
  });

  it("F-DI-09 turning sync off says what stays on the server and can delete it", async () => {
    const calls = fakeBackend({
      dictionary_list: () => [],
      dictionary_suggestions: () => [],
      dictionary_sync: () => ({ pushed: 0, pulled: 0, conflicts: [], syncedAt: 1_700_000_000_000 }),
      dictionary_sync_forget: () => null,
    });
    proSync();
    act(() => useSettings.setState({ values: { ...useSettings.getState().values, dictate_dictionary_sync: true } }));
    wrap(<DictionarySheet open onClose={() => undefined} />);
    fireEvent.click(await screen.findByRole("switch", { name: "Sync dictionary" }));
    expect(await screen.findByText(/The synced copy stays in your Vibezeo account until you delete it/)).toBeTruthy();
    fireEvent.click(screen.getByTestId("dictionary-sync-off-delete"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictionary_sync_forget")).toBe(true));
    expect(
      calls.some((c) => c.cmd === "settings_set" && c.args?.key === "dictate_dictionary_sync" && c.args?.value === false),
    ).toBe(true);
  });
});

const MODELS: ModelsView = {
  usedBytes: 0,
  active: null,
  pending: null,
  models: [
    {
      id: "whisper-base.en",
      name: "Whisper Base (English)",
      bytes: 147964211,
      langsLabel: "English",
      languages: ["en"],
      recommended: false,
      available: true,
      cpuWarning: false,
      runnable: true,
      state: "failed",
      progress: null,
      error: "Download failed: the file didn't match.",
      license: "MIT",
    },
    {
      id: "distil-whisper-large-v3.5",
      name: "Distil-Whisper (English)",
      bytes: 1519521155,
      langsLabel: "English",
      languages: ["en"],
      recommended: false,
      available: true,
      cpuWarning: true,
      runnable: true,
      state: "downloading",
      progress: { stage: "downloading", done: 759760577, total: 1519521155, speed: 9000000 },
      error: null,
      license: "MIT",
    },
  ],
};

describe("Model manager (F-DI-04, UI-63)", () => {
  it("F-DI-04.1 a failed download says the file didn't match and offers Retry", async () => {
    const calls = fakeBackend({ dictate_models: () => MODELS });
    wrap(<ModelManager open onClose={() => undefined} />);
    await waitFor(() =>
      expect(screen.getByTestId("model-failed-whisper-base.en").textContent).toContain(
        "Download failed: the file didn't match.",
      ),
    );
    expect(screen.getByText("CPU transcription")).toBeTruthy();
    expect(screen.getByTestId("model-progress-distil-whisper-large-v3.5").textContent).toContain("50%");
    fireEvent.click(screen.getByTestId("model-retry-whisper-base.en"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictate_model_download")?.args).toEqual({
        modelId: "whisper-base.en",
      }),
    );
  });
});

describe("Settings › Dictate (UI-62)", () => {
  it("UI-62 loading, then the home card asks to finish setup until the key is verified", async () => {
    fakeBackend({
      dictate_status: () => STATUS,
      dictate_models: () => MODELS,
      dictate_stats: () => ({ words: 63410, speakingMs: 20_880_000, takes: 2190, wpm: 164, savedMs: 0 }),
      dictionary_list: () => [{}, {}],
    });
    wrap(<DictateExtras />);
    expect(screen.getByTestId("dictate-loading")).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId("dictate-home").getAttribute("data-ready")).toBe("false"));
    expect(screen.getByText("Finish Dictate setup")).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId("dictate-stats").textContent).toContain("63,410 words"));
    expect(screen.getByText("Dictionary (2 rules)")).toBeTruthy();
    act(() => useDictate.setState({ status: { ...STATUS, verified: true } }));
    expect(screen.getByText("Ready to dictate")).toBeTruthy();
  });
});

describe("Settings › Dictate error state", () => {
  it("UI-62 a core that can't answer shows the error state, not a spinner forever", async () => {
    fakeBackend({ dictate_status: () => null });
    wrap(<DictateExtras />);
    await waitFor(() => expect(screen.getByTestId("dictate-unavailable")).toBeTruthy());
  });

  it("UI-62 a core that has no Dictate at all says why, in its own words (the browser demo)", async () => {
    useDictate.setState({ status: null, unavailable: null });
    fakeBackend({
      dictate_status: () => {
        throw { code: "unavailable", message: "Dictate listens to your microphone, so it works in the app." };
      },
    });
    wrap(<DictateExtras />);
    await waitFor(() =>
      expect(screen.getByTestId("dictate-unavailable").textContent).toBe(
        "Dictate listens to your microphone, so it works in the app.",
      ),
    );
  });
});

describe("Cloud transcription (F-DI-05)", () => {
  it("F-DI-05.2 a cloud failure shows the server's words and offers Use local for this take", async () => {
    const calls = fakeBackend({ dictate_status: () => STATUS, dictate_use_local: () => true });
    wrap(<DictateWidget />);
    act(() =>
      emitLocal("dictate:toast", {
        code: "cloud_failed",
        error: true,
        app: null,
        model: null,
        previous: null,
        detail: "HTTP 402 CREDITS_EXHAUSTED",
        message: "Out of credits for cloud transcription",
        actions: ["use_local"],
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("widget-text").textContent).toBe("Out of credits for cloud transcription"),
    );
    expect(calls.some((c) => c.cmd === "dictate_use_local")).toBe(false);
    fireEvent.click(screen.getByTestId("toast-action-use_local"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_use_local")).toBe(true));
  });

  it("F-DI-05 with no account Cloud stays put (it comes with accounts); with it, asks once before sending audio", async () => {
    const calls = fakeBackend();
    wrap(<EngineControl />);
    const cloud = () => screen.getByRole("radio", { name: /Cloud/ });
    fireEvent.click(cloud());
    expect(screen.getByTestId("dictate-engine").getAttribute("data-engine")).toBe("local");
    expect(calls.some((c) => c.cmd === "settings_set" && c.args?.key === "dictate_engine")).toBe(false);
    act(() => {
      const e = useSession.getState().entitlement!;
      useSession.setState({ entitlement: { ...e, limits: { ...e.limits, cloudStt: true } } });
    });
    fireEvent.click(cloud());
    fireEvent.click(await screen.findByRole("button", { name: "Use cloud" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "settings_set").map((c) => [c.args?.key, c.args?.value])).toEqual([
        ["dictate_cloud_consented", true],
        ["dictate_engine", "cloud"],
      ]),
    );
    await waitFor(() => expect(useSettings.getState().values.dictate_engine).toBe("cloud"));
    expect(screen.getByTestId("dictate-engine-help").textContent).toContain("Audio is sent");
    expect(screen.getByText("Share dictionary hints with cloud")).toBeTruthy();
  });
});

describe("Enhance and Polish (F-DI-10)", () => {
  const inserted = {
    state: "inserted" as const,
    takeId: "take_1",
    handsFree: false,
    composer: null,
    words: 2,
  };

  it("F-DI-10 the first quick action shows the disclosure, then sends with it acknowledged", async () => {
    const calls = fakeBackend({
      dictate_status: () => ({ ...STATUS, quickActions: true }),
      dictate_quick_action: () => true,
    });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() => emitLocal("dictate:state", inserted));
    fireEvent.click(await screen.findByTestId("widget-enhance"));
    expect(screen.getByTestId("widget-text").textContent).toContain("not audio");
    expect(calls.some((c) => c.cmd === "dictate_quick_action")).toBe(false);
    fireEvent.click(screen.getByTestId("widget-disclosure-continue"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictate_quick_action")?.args).toEqual({
        mode: "enhance",
        disclosed: true,
      }),
    );
  });

  it("F-DI-10 once disclosed, Polish sends straight away and shows Polishing with its cost", async () => {
    const calls = fakeBackend({
      dictate_status: () => ({ ...STATUS, quickActions: true, enhanceDisclosed: true }),
      dictate_quick_action: () => true,
    });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() => emitLocal("dictate:state", inserted));
    fireEvent.click(await screen.findByTestId("widget-polish"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictate_quick_action")?.args).toEqual({ mode: "polish" }),
    );
    act(() => emitLocal("dictate:state", { ...inserted, state: "polishing", words: null }));
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Polishing…"));
    expect(screen.getByText("◈ 3")).toBeTruthy();
  });

  it("F-DI-10 no quick actions without the setting or an account that carries Enhance", async () => {
    fakeBackend({ dictate_status: () => STATUS });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() => emitLocal("dictate:state", inserted));
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Inserted · 2 words"));
    expect(screen.queryByTestId("widget-quick-actions")).toBeNull();
  });

  it("F-DI-10.1 F-DI-10.2 the toasts say what happened to the text", () => {
    const base = {
      error: false,
      app: null,
      model: null,
      previous: null,
      detail: "",
      message: null,
      actions: [],
    };
    expect(toastText({ ...base, code: "too_long_to_enhance" })).toBe(
      "That's too long to enhance (5,000 characters max).",
    );
    expect(toastText({ ...base, code: "enhance_failed" })).toBe(
      "Enhance didn't work, so your exact words were used.",
    );
  });

  it("F-DI-10 Auto-Enhance asks once before any text is sent; with no account (hosted, D-2903) it can't turn on", async () => {
    const calls = fakeBackend();
    const { unmount } = wrap(<AutoEnhanceControl label="Enhance every take automatically" />);
    expect(
      (screen.getByRole("switch", { name: "Enhance every take automatically" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    unmount();
    useSession.setState({ entitlement: withEnhance() });
    wrap(<AutoEnhanceControl label="Enhance every take automatically" />);
    fireEvent.click(screen.getByRole("switch", { name: "Enhance every take automatically" }));
    expect(calls.some((c) => c.cmd === "settings_set")).toBe(false);
    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "settings_set").map((c) => [c.args?.key, c.args?.value])).toEqual([
        ["dictate_enhance_disclosed", true],
        ["dictate_auto_enhance", true],
      ]),
    );
  });

  it("F-DI-10 formatting instructions stop at 2,000 characters", async () => {
    fakeBackend();
    useSession.setState({ entitlement: withEnhance() });
    wrap(<FormattingControl label="Formatting instructions" />);
    const box = screen.getByRole("textbox", { name: "Formatting instructions" });
    fireEvent.change(box, { target: { value: "x".repeat(2_500) } });
    expect(screen.getByTestId("dictate-formatting-count").textContent).toBe("2,000/2,000");
  });
});

describe("Setup without a global hotkey (F-DI-11, D-515)", () => {
  it("F-DI-11 on Wayland without the portal, Try it names the command and the link verifies", async () => {
    fakeBackend({ dictate_status: () => ({ ...STATUS, verified: true }) });
    useDictate.setState({
      status: { ...STATUS, session: "wayland", deepLinkCommand: "xdg-open tandem://dictate/toggle" },
    });
    wrap(<TryIt />);
    fireEvent.click(screen.getByTestId("dictate-verify"));
    expect(screen.getByTestId("dictate-try-link").textContent).toContain("xdg-open tandem://dictate/toggle");
    act(() => emitLocal("dictate:verified", null));
    await waitFor(() => expect(screen.getByTestId("dictate-key-detected")).toBeTruthy());
    expect(screen.queryByTestId("dictate-try-link")).toBeNull();
  });
});

describe("macOS Fn key (F-DI-02, F-DI-11, D-524)", () => {
  const MAC = { ...STATUS, session: "mac" as const, hold: { binding: "Fn", label: "Fn" } };

  it("F-DI-02 an unheard Fn says the key became Control+Option, and Use Fn again goes back", async () => {
    const calls = fakeBackend({ dictate_set_binding: () => ({ kind: "active" }), dictate_status: () => MAC });
    useDictate.setState({
      status: { ...MAC, fnFallback: true, hold: { binding: "Ctrl+Alt", label: "⌃⌥" } },
    });
    wrap(<TryIt />);
    expect(screen.getByTestId("dictate-fn-fallback").textContent).toContain("Control+Option");
    expect(screen.queryByText(/Some keyboards don't send Fn/)).toBeNull();
    fireEvent.click(screen.getByTestId("dictate-fn-again"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictate_set_binding")?.args).toEqual({ binding: "Fn", mode: "hold" }),
    );
  });

  it("F-DI-02 Use Control+Option and the notice after Fn went unheard have their own words (D-866)", () => {
    fakeBackend({ dictate_status: () => MAC });
    useBoot.setState({ boot: { ...BOOT, platform: "macos" } });
    useDictate.setState({
      status: { ...MAC, fnFallback: true, hold: { binding: "Ctrl+Alt", label: "⌃⌥" } },
    });
    wrap(
      <>
        <ShortcutChoice />
        <TryIt />
      </>,
    );
    expect(screen.getByRole("button", { name: "Fn not working? Use Control+Option" })).toBeTruthy();
    expect(screen.getByTestId("dictate-fn-fallback").textContent).toContain("Fn didn't reach Vibezeo");
  });

  it("F-DI-11 the 🌐 key changes only after consent, and can be put back", async () => {
    let usage: string | null = "show_emoji";
    let changed = false;
    const calls = fakeBackend({
      dictate_globe_key: () => ({ available: true, usage, changedByApp: changed }),
      dictate_globe_key_set: (a) => {
        const doNothing = (a as { doNothing: boolean }).doNothing;
        usage = doNothing ? "do_nothing" : "show_emoji";
        changed = doNothing;
        return { available: true, usage, changedByApp: changed };
      },
    });
    wrap(<GlobeKey />);
    await waitFor(() => expect(screen.getByTestId("dictate-globe").getAttribute("data-usage")).toBe("show_emoji"));
    fireEvent.click(screen.getByTestId("dictate-globe-change"));
    // Opening the dialog changes nothing; it names the current value.
    expect(calls.some((c) => c.cmd === "dictate_globe_key_set")).toBe(false);
    expect(screen.getByText(/It's set to Show Emoji & Symbols now/)).toBeTruthy();
    fireEvent.click(screen.getByText("Set to Do Nothing"));
    await waitFor(() => expect(screen.getByTestId("dictate-globe").getAttribute("data-usage")).toBe("do_nothing"));
    expect(calls.find((c) => c.cmd === "dictate_globe_key_set")?.args).toEqual({ doNothing: true });
    fireEvent.click(screen.getByTestId("dictate-globe-undo"));
    await waitFor(() => expect(screen.getByTestId("dictate-globe").getAttribute("data-usage")).toBe("show_emoji"));
    expect(screen.getByTestId("dictate-globe-change")).toBeTruthy();
  });

  it("F-DI-11 off macOS the 🌐 row isn't there", async () => {
    const calls = fakeBackend({ dictate_globe_key: () => ({ available: false, usage: null, changedByApp: false }) });
    wrap(<GlobeKey />);
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_globe_key")).toBe(true));
    expect(screen.queryByTestId("dictate-globe")).toBeNull();
  });
});

describe("macOS permissions (F-DI-03, D-2108)", () => {
  it("F-DI-03 the microphone has a row, Grant goes to the core, and Quit & Reopen waits for the hotkey", async () => {
    const calls = fakeBackend({
      dictate_permissions: () => ({ mic: "unknown", inputMonitoring: "granted", accessibility: "denied" }),
    });
    useDictate.setState({ status: STATUS });
    wrap(<Permissions />);
    await waitFor(() => expect(screen.getByTestId("dictate-grant-microphone")).toBeTruthy());
    expect(screen.getByTestId("dictate-grant-accessibility")).toBeTruthy();
    expect(screen.queryByTestId("dictate-grant-input_monitoring")).toBeNull();
    // The hotkey already runs: nothing to reopen for.
    expect(screen.queryByText("Quit & Reopen")).toBeNull();
    fireEvent.click(screen.getByTestId("dictate-grant-microphone"));
    await waitFor(() =>
      expect(
        calls.some((c) => c.cmd === "dictate_open_permission_settings" && c.args?.kind === "microphone"),
      ).toBe(true),
    );
    act(() => useDictate.setState({ status: { ...STATUS, hotkey: { kind: "needs_permission" } } }));
    await waitFor(() => expect(screen.getByText("Quit & Reopen")).toBeTruthy());
  });

  it("F-DI-03 shows nothing where the OS has no such permissions", async () => {
    const calls = fakeBackend({
      dictate_permissions: () => ({
        mic: "not_needed",
        inputMonitoring: "not_needed",
        accessibility: "not_needed",
      }),
    });
    useDictate.setState({ status: STATUS });
    const { container } = wrap(<Permissions />);
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_permissions")).toBe(true));
    expect(container.textContent).toBe("");
  });
});

describe("Dictate's status lines say what's true (D-520)", () => {
  it("F-DI-05 with Cloud chosen, no local model is needed: the status bar shows the key", async () => {
    fakeBackend({});
    useDictate.setState({ status: { ...STATUS, model: null, engine: "cloud" } });
    wrap(<StatusBar />);
    expect(screen.getByTestId("status-dictate").textContent).toBe("Dictate: Ctrl+Alt");
    useDictate.setState({ status: { ...STATUS, model: null, engine: "local" } });
    await waitFor(() =>
      expect(screen.getByTestId("status-dictate").textContent).toBe("Dictate: download a model"),
    );
  });

  it("F-DI-05 the home card's Cloud chip names the language chosen", async () => {
    fakeBackend({ dictate_stats: () => ({ words: 0, speakingMs: 0, takes: 0, wpm: 0, savedMs: 0 }) });
    useDictate.setState({ status: { ...STATUS, engine: "cloud" } });
    useSettings.setState({ values: { dictate_language: "de" } });
    const { rerender } = wrap(<DictateHomeCard onTry={() => undefined} onHistory={() => undefined} />);
    await waitFor(() => expect(screen.getByText(/^Cloud · /)).toBeTruthy());
    expect(screen.getByText(/^Cloud · /).textContent).toBe("Cloud · German");
    act(() => useSettings.setState({ values: { dictate_language: "auto" } }));
    rerender(
      <UiProvider>
        <DictateHomeCard onTry={() => undefined} onHistory={() => undefined} />
      </UiProvider>,
    );
    await waitFor(() => expect(screen.getByText(/^Cloud · /).textContent).toBe("Cloud · Auto-detect"));
  });
});
