import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { DictateStatus, ModelsView, TakePage } from "@tandem/protocol";
import { Composer, UiProvider, type ComposerHandle } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { useDictate, watchDictate } from "../../stores/dictate";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { bindingFrom, keyName, presets } from "./binding";
import { DictateExtras, MicSelect, ShortcutField } from "./DictatePage";
import { EngineControl } from "./EngineControl";
import { AutoEnhanceControl, FormattingControl } from "./EnhanceControls";
import { useSettings } from "../../stores/settings";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { Permissions, TryIt } from "./DictateSetup";
import { DictionarySheet } from "./DictionarySheet";
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
};

const wrap = (ui: React.ReactNode) => render(<UiProvider>{ui}</UiProvider>);

describe("Dictate bindings (F-DI-01)", () => {
  it("F-DI-01 recorded keys become binding strings, modifier-only included", () => {
    expect(bindingFrom({ code: "", ctrlKey: true, altKey: true, shiftKey: false, metaKey: false })).toBe("Ctrl+Alt");
    expect(bindingFrom({ code: "KeyD", ctrlKey: true, altKey: false, shiftKey: true, metaKey: false })).toBe("Ctrl+Shift+D");
    expect(bindingFrom({ code: "Mouse4", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false })).toBe("Mouse4");
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
      expect(screen.getByTestId("dictate-shortcut-error-toggle").textContent).toBe("Toggle needs a key that isn't only modifiers."),
    );
  });
});

describe("Dictate widget (F-DI-07, UI-60)", () => {
  it("F-DI-07 listening shows the timer and ×, hands-free names the key, insertion counts words", async () => {
    fakeBackend({ dictate_status: () => STATUS });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() => emitLocal("dictate:state", { state: "listening", takeId: "take_1", handsFree: true, composer: null, words: null }));
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Hands-free · press Ctrl+Alt to stop"));
    expect(screen.getByTestId("widget-timer").textContent).toBe("0:00");
    expect(screen.getByRole("button", { name: "Discard this take" })).toBeTruthy();
    act(() => emitLocal("dictate:state", { state: "inserted", takeId: "take_1", handsFree: false, composer: null, words: 42 }));
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
        detail: "No samples arrived within 1 s, after rebuilding the stream and trying the backup microphone.",
        message: null,
        actions: [],
      }),
    );
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Your microphone isn't delivering audio"));
    const pill = screen.getByTestId("dictate-widget");
    expect(pill.getAttribute("title")).toBe("No samples arrived within 1 s, after rebuilding the stream and trying the backup microphone.");
    expect(pill.getAttribute("data-state")).toBe("error");
  });

  it("F-DI-07 toasts read from the catalog; composer takes toast in the main window", () => {
    const t = { code: "elevated" as const, error: false, app: "Registry Editor", model: null, previous: null, detail: "", message: null, actions: [] };
    expect(toastText(t)).toBe("Registry Editor is running as administrator, so Vibezeo can't type into it. Copied instead.");
    expect(mainWindowShows({ ...t, code: "no_speech" }, true, false)).toBe(true);
    expect(mainWindowShows({ ...t, code: "no_speech" }, false, false)).toBe(false);
    expect(mainWindowShows({ ...t, code: "model_fallback" }, false, false)).toBe(true);
  });
});

function DictatedComposer({ handle }: { handle: React.RefObject<ComposerHandle | null> }) {
  const [value, setValue] = useStateValue("fix the  test");
  const { dictation, onFocusChange } = useComposerDictation("thread:t1", handle);
  return (
    <Composer ref={handle} value={value} onChange={setValue} onSend={() => undefined} placeholder="Message" dictation={dictation} onFocusChange={onFocusChange} />
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
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_composer_focus" && c.args?.targetId === "thread:t1")).toBe(true));
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
      expect(calls.filter((c) => c.cmd === "dictate_start_take")[0]?.args).toEqual({ origin: { type: "composer", targetId: "thread:t1" } }),
    );
    expect(calls.some((c) => c.cmd === "dictate_stop_take")).toBe(false);
    act(() => emitLocal("dictate:state", { state: "listening", takeId: "take_1", handsFree: true, composer: "thread:t1", words: null }));
    await waitFor(() => expect(screen.getByTestId("composer-mic").getAttribute("data-state")).toBe("listening"));
    expect((screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement).placeholder).toBe("Listening…");
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
      dictate_devices: () => ({ devices: [{ id: "mic1", name: "Desk Mic", isDefault: true, bluetooth: false, loopback: false }], chosen: null }),
    });
    wrap(
      <>
        <MicSelect k="dictate_mic" label="Microphone" none="None" />
        <MicSelect k="dictate_backup_mic" label="Backup microphone" none="None" />
      </>,
    );
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Microphone" }).textContent).toContain("System default (Desk Mic)"));
    expect(screen.getByRole("combobox", { name: "Backup microphone" }).textContent).toContain("None");
  });
});

const page = (takes: TakePage["takes"]): TakePage => ({ takes, next: null });

describe("History and stats (F-DI-08, UI-65)", () => {
  it("F-DI-08.1 search asks the core, export writes the CSV, clear confirms", async () => {
    const take = { id: "take_1", createdAt: 0, text: "refactor the auth guard", engine: "local", model: "whisper-base.en", wordCount: 4, durationMs: 3000, appId: null };
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
    await waitFor(() => expect(calls.find((c) => c.cmd === "dictate_history_export")?.args).toEqual({ path: "/tmp/history.csv" }));
    fireEvent.click(screen.getByTestId("history-clear"));
    fireEvent.click(await screen.findByRole("button", { name: "Clear history" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_history_clear")).toBe(true));
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
    fireEvent.change(screen.getByTestId("rule-phrases").querySelector("textarea") ?? screen.getByTestId("rule-phrases"), { target: { value: "tandem" } });
    fireEvent.change(screen.getByTestId("rule-replacement").querySelector("input") ?? screen.getByTestId("rule-replacement"), { target: { value: "Tandem" } });
    fireEvent.click(screen.getByTestId("rule-save"));
    await waitFor(() => {
      const saved = calls.find((c) => c.cmd === "dictionary_save")?.args?.rule as { kind: string; phrases: string[]; replacement: string } | undefined;
      expect(saved?.kind).toBe("correction");
      expect(saved?.phrases).toEqual(["tandem"]);
      expect(saved?.replacement).toBe("Tandem");
    });
    const box = screen.getByTestId("dictionary-test-input");
    fireEvent.change(box.querySelector("input") ?? box, { target: { value: "use tandem, https://tandem.dev" } });
    await waitFor(() => expect(screen.getByTestId("dictionary-test-output").textContent).toBe("use Tandem, https://tandem.dev"));
  });
});

const MODELS: ModelsView = {
  usedBytes: 0,
  active: null,
  pending: null,
  models: [
    { id: "whisper-base.en", name: "Whisper Base (English)", bytes: 147964211, langsLabel: "English", languages: ["en"], recommended: false, available: true, cpuWarning: false, runnable: true, state: "failed", progress: null, error: "Download failed: the file didn't match.", license: "MIT" },
    { id: "distil-whisper-large-v3.5", name: "Distil-Whisper (English)", bytes: 1519521155, langsLabel: "English", languages: ["en"], recommended: false, available: true, cpuWarning: true, runnable: true, state: "downloading", progress: { stage: "downloading", done: 759760577, total: 1519521155, speed: 9000000 }, error: null, license: "MIT" },
  ],
};

describe("Model manager (F-DI-04, UI-63)", () => {
  it("F-DI-04.1 a failed download says the file didn't match and offers Retry", async () => {
    const calls = fakeBackend({ dictate_models: () => MODELS });
    wrap(<ModelManager open onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("model-failed-whisper-base.en").textContent).toContain("Download failed: the file didn't match."));
    expect(screen.getByText("CPU transcription")).toBeTruthy();
    expect(screen.getByTestId("model-progress-distil-whisper-large-v3.5").textContent).toContain("50%");
    fireEvent.click(screen.getByTestId("model-retry-whisper-base.en"));
    await waitFor(() => expect(calls.find((c) => c.cmd === "dictate_model_download")?.args).toEqual({ modelId: "whisper-base.en" }));
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
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Out of credits for cloud transcription"));
    expect(calls.some((c) => c.cmd === "dictate_use_local")).toBe(false);
    fireEvent.click(screen.getByTestId("toast-action-use_local"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_use_local")).toBe(true));
  });

  it("F-DI-05 Free opens the plans; Pro asks once before sending audio", async () => {
    const calls = fakeBackend();
    useSession.setState({ entitlement: { ...useSession.getState().entitlement!, plan: "free" } });
    wrap(<EngineControl />);
    const cloud = () => screen.getByRole("radio", { name: /Cloud/ });
    fireEvent.click(cloud());
    expect(useUi.getState().upgrade.open).toBe(true);
    expect(calls.some((c) => c.cmd === "settings_set" && c.args?.key === "dictate_engine")).toBe(false);
    act(() => useUi.setState({ upgrade: { open: false } }));
    act(() => useSession.setState({ entitlement: { ...useSession.getState().entitlement!, plan: "pro" } }));
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
  const inserted = { state: "inserted" as const, takeId: "take_1", handsFree: false, composer: null, words: 2 };

  it("F-DI-10 the first quick action shows the disclosure, then sends with it acknowledged", async () => {
    const calls = fakeBackend({ dictate_status: () => ({ ...STATUS, quickActions: true }), dictate_quick_action: () => true });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() => emitLocal("dictate:state", inserted));
    fireEvent.click(await screen.findByTestId("widget-enhance"));
    expect(screen.getByTestId("widget-text").textContent).toContain("not audio");
    expect(calls.some((c) => c.cmd === "dictate_quick_action")).toBe(false);
    fireEvent.click(screen.getByTestId("widget-disclosure-continue"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "dictate_quick_action")?.args).toEqual({ mode: "enhance", disclosed: true }),
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
    await waitFor(() => expect(calls.find((c) => c.cmd === "dictate_quick_action")?.args).toEqual({ mode: "polish" }));
    act(() => emitLocal("dictate:state", { ...inserted, state: "polishing", words: null }));
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Polishing…"));
    expect(screen.getByText("◈ 3")).toBeTruthy();
  });

  it("F-DI-10 no quick actions without the setting or Pro", async () => {
    fakeBackend({ dictate_status: () => STATUS });
    wrap(<DictateWidget />);
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Vibezeo"));
    act(() => emitLocal("dictate:state", inserted));
    await waitFor(() => expect(screen.getByTestId("widget-text").textContent).toBe("Inserted · 2 words"));
    expect(screen.queryByTestId("widget-quick-actions")).toBeNull();
  });

  it("F-DI-10.1 F-DI-10.2 the toasts say what happened to the text", () => {
    const base = { error: false, app: null, model: null, previous: null, detail: "", message: null, actions: [] };
    expect(toastText({ ...base, code: "too_long_to_enhance" })).toBe("That's too long to enhance (5,000 characters max).");
    expect(toastText({ ...base, code: "enhance_failed" })).toBe("Enhance didn't work, so your exact words were used.");
  });

  it("F-DI-10 Auto-Enhance asks once before any text is sent; Free can't turn it on", async () => {
    const calls = fakeBackend();
    useSession.setState({ entitlement: { ...useSession.getState().entitlement!, plan: "free" } });
    const { unmount } = wrap(<AutoEnhanceControl label="Enhance every take automatically" />);
    expect((screen.getByRole("switch", { name: "Enhance every take automatically" }) as HTMLButtonElement).disabled).toBe(true);
    unmount();
    useSession.setState({ entitlement: { ...useSession.getState().entitlement!, plan: "pro" } });
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
    useSession.setState({ entitlement: { ...useSession.getState().entitlement!, plan: "pro" } });
    wrap(<FormattingControl label="Formatting instructions" />);
    const box = screen.getByRole("textbox", { name: "Formatting instructions" });
    fireEvent.change(box, { target: { value: "x".repeat(2_500) } });
    expect(screen.getByTestId("dictate-formatting-count").textContent).toBe("2,000/2,000");
  });
});

describe("Setup without a global hotkey (F-DI-11, D-515)", () => {
  it("F-DI-11 on Wayland without the portal, Try it names the command and the link verifies", async () => {
    fakeBackend({ dictate_status: () => ({ ...STATUS, verified: true }) });
    useDictate.setState({ status: { ...STATUS, session: "wayland", deepLinkCommand: "xdg-open tandem://dictate/toggle" } });
    wrap(<TryIt />);
    fireEvent.click(screen.getByTestId("dictate-verify"));
    expect(screen.getByTestId("dictate-try-link").textContent).toContain("xdg-open tandem://dictate/toggle");
    act(() => emitLocal("dictate:verified", null));
    await waitFor(() => expect(screen.getByTestId("dictate-key-detected")).toBeTruthy());
    expect(screen.queryByTestId("dictate-try-link")).toBeNull();
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
      expect(calls.some((c) => c.cmd === "dictate_open_permission_settings" && c.args?.kind === "microphone")).toBe(true),
    );
    act(() => useDictate.setState({ status: { ...STATUS, hotkey: { kind: "needs_permission" } } }));
    await waitFor(() => expect(screen.getByText("Quit & Reopen")).toBeTruthy());
  });

  it("F-DI-03 shows nothing where the OS has no such permissions", async () => {
    const calls = fakeBackend({
      dictate_permissions: () => ({ mic: "not_needed", inputMonitoring: "not_needed", accessibility: "not_needed" }),
    });
    useDictate.setState({ status: STATUS });
    const { container } = wrap(<Permissions />);
    await waitFor(() => expect(calls.some((c) => c.cmd === "dictate_permissions")).toBe(true));
    expect(container.textContent).toBe("");
  });
});
