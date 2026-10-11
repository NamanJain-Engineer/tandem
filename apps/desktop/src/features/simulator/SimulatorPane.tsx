import { memo, useCallback, useEffect, useReducer, useRef, useState } from "react";
import { useDraggable } from "@dnd-kit/core";
import { Smartphone } from "lucide-react";
import type { IpcError, PaneRecord, SimDevice, SimScreen, SimStatus } from "@tandem/protocol";
import { Button, cx, Select, Spinner, useToast, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useCloseConfirm } from "../../stores/closeConfirm";
import { useFlash } from "../../stores/focus";
import { useLayout } from "../../stores/layout";
import { createPngChannel, useSimulator } from "../../stores/simulator";
import { useShortcutKeys } from "../commands/useShortcutKeys";
import { DropZones, moveToNewTab, type PaneDrag } from "../code/dnd";
import { useDropTarget } from "../code/drops";
import { PaneChrome } from "../code/PaneChrome";
import { deviceOf, initialSimPane, reduceSim, simInputFor, toDevicePoint } from "./model";

type Props = {
  tabId: string;
  pane: PaneRecord;
  title: string;
  visible: boolean;
  focused: boolean;
  solo: boolean;
  maximized: boolean;
  cover: "card" | "line" | null;
};

const message = (e: unknown) => (e as IpcError)?.message ?? t("sim.failed");

/** The Simulator pane (F-CD-21, macOS): device and runtime pickers, Boot and
 * Shutdown, the device's screen at 10 fps, and clicks and typing through idb. */
export const SimulatorPane = memo(function SimulatorPane({ tabId, pane, title, visible, focused, solo, maximized, cover }: Props) {
  const keys = useShortcutKeys();
  const { show } = useToast();
  const layout = useLayout.getState;
  const dropping = useDropTarget((st) => st.paneId === pane.id);
  const flashing = useFlash((st) => st.flashing[pane.id] !== undefined);
  const drag: PaneDrag = { kind: "pane", paneId: pane.id, tabId, title };
  const { attributes, listeners, setNodeRef } = useDraggable({ id: `pane:${pane.id}`, data: drag });
  const [state, dispatch] = useReducer(reduceSim, initialSimPane);
  const [frame, setFrame] = useState<string | null>(null);
  const [screen, setScreen] = useState<SimScreen | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const device = deviceOf(state);
  const input = state.status?.input ?? false;

  const refresh = useCallback(async () => {
    const status = await useSimulator.getState().load();
    if (status) dispatch({ type: "status", status });
    else dispatch({ type: "status_failed", message: t("sim.unavailable") });
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Frames while the device is live and the pane can be seen.
  const udid = device?.udid ?? null;
  const live = state.phase === "live" && visible && cover === null && udid !== null;
  useEffect(() => {
    if (!live || !udid) return;
    let url: string | null = null;
    let stopped = false;
    void createPngChannel((png) => {
      if (stopped) return;
      const next = URL.createObjectURL(new Blob([png], { type: "image/png" }));
      setFrame(next);
      if (url) URL.revokeObjectURL(url);
      url = next;
    }).then((channel) =>
      invoke("sim_stream_start", { paneId: pane.id, udid, channel }).catch((e: unknown) =>
        dispatch({ type: "failed", message: message(e) }),
      ),
    );
    return () => {
      stopped = true;
      void invoke("sim_stream_stop", { paneId: pane.id }).catch(() => undefined);
      if (url) URL.revokeObjectURL(url);
    };
  }, [live, udid, pane.id]);

  useEffect(() => {
    if (state.phase !== "live" || !udid || !input) return;
    let gone = false;
    void invoke<SimScreen>("sim_screen", { udid })
      .then((s) => !gone && setScreen(s))
      .catch(() => undefined);
    return () => {
      gone = true;
    };
  }, [state.phase, udid, input]);

  const boot = async () => {
    if (!udid) return;
    dispatch({ type: "boot" });
    try {
      await invoke<SimDevice>("sim_boot", { udid }, { timeoutMs: 200_000 });
      dispatch({ type: "booted" });
    } catch (e) {
      dispatch({ type: "failed", message: message(e) });
    }
  };
  const shutdown = async () => {
    if (!udid) return;
    dispatch({ type: "shutdown" });
    setFrame(null);
    try {
      await invoke<SimDevice>("sim_shutdown", { udid });
      dispatch({ type: "stopped" });
    } catch (e) {
      dispatch({ type: "failed", message: message(e) });
    }
  };
  const install = async () => {
    if (!udid) return;
    try {
      const done = await invoke<boolean>("sim_install", { udid }, { timeoutMs: 600_000 });
      if (done) show({ tone: "success", title: t("sim.installed") });
    } catch (e) {
      show({ tone: "error", title: message(e) });
    }
  };
  const send = (cmd: string, args: Record<string, unknown>) =>
    void invoke(cmd, { udid, ...args }).catch((e: unknown) => show({ tone: "error", title: message(e) }));

  const onClick = (e: React.MouseEvent<HTMLImageElement>) => {
    const img = imgRef.current;
    if (!input || !screen || !img) return;
    const p = toDevicePoint(e.clientX, e.clientY, img.getBoundingClientRect(), { width: img.naturalWidth, height: img.naturalHeight }, screen);
    if (p) send("sim_tap", p);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!input) return;
    const what = simInputFor(e);
    if (!what) return;
    e.preventDefault();
    send("key" in what ? "sim_key" : "sim_text", what);
  };

  const split = (side: "right" | "bottom") => {
    if (layout().requestSplit(tabId, pane.id, side) === "no_room") show({ tone: "warning", title: t("split.noRoom") });
  };
  const menu: MenuEntry[] = [
    { id: "title", type: "label", label: title },
    { id: "split-right", label: t("pane.splitRight"), kbd: keys("pane.split_right"), onSelect: () => split("right") },
    { id: "split-down", label: t("pane.splitDown"), kbd: keys("pane.split_down"), onSelect: () => split("bottom") },
    { id: "new-tab", label: t("pane.moveToNewTab"), onSelect: () => void moveToNewTab(tabId, pane.id) },
    {
      id: "close",
      label: t("pane.closePane"),
      danger: true,
      kbd: keys("pane.close"),
      onSelect: () => useCloseConfirm.getState().request(tabId, pane.id),
    },
  ];

  return (
    <section
      aria-label={t("pane.label", { name: title })}
      data-testid="pane"
      data-pane-id={pane.id}
      data-kind="simulator"
      data-focused={focused ? "true" : undefined}
      data-drop-target={dropping ? "true" : undefined}
      data-flash={flashing ? "true" : undefined}
      className={cx(
        "relative flex h-full min-h-0 flex-col overflow-hidden rounded-md border",
        flashing && "tn-flash",
        dropping && "ring-2 ring-accent",
        focused && !solo ? "border-border-strong" : "border-border",
      )}
      onFocusCapture={() => layout().focus(tabId, pane.id)}
      onPointerDownCapture={() => layout().focus(tabId, pane.id)}
    >
      <PaneChrome
        title={title}
        status="idle"
        focused={focused && !solo}
        solo={solo}
        maximized={maximized}
        menuItems={menu}
        onMaximize={() => layout().toggleMaximize(tabId, pane.id)}
        onClose={() => useCloseConfirm.getState().request(tabId, pane.id)}
        glyph={<Smartphone size={13} />}
        dragRef={setNodeRef}
        dragProps={{ ...attributes, ...listeners, tabIndex: -1, role: "toolbar" }}
      />
      <div className="relative flex min-h-0 flex-1 flex-col" data-testid="sim-body" data-phase={state.phase}>
        {cover && (
          <button
            type="button"
            data-testid={cover === "card" ? "pane-card" : "pane-collapsed"}
            onClick={() => layout().flipCard(tabId, pane.id)}
            className="absolute inset-0 z-10 flex items-center justify-center gap-2 bg-surface-2 text-sm text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            <Smartphone size={14} aria-hidden="true" className="text-fg-muted" />
            <span className="truncate">{title}</span>
          </button>
        )}
        {state.status?.available && state.status.devices.length > 0 && (
          <Toolbar
            status={state.status}
            udid={udid}
            phase={state.phase}
            onSelect={(id) => dispatch({ type: "select", udid: id })}
            onBoot={() => void boot()}
            onShutdown={() => void shutdown()}
            onHome={() => send("sim_home", {})}
            onInstall={() => void install()}
          />
        )}
        <div className="flex min-h-0 flex-1 items-center justify-center bg-surface-1 p-2">
          {state.phase === "loading" && (
            <p className="flex items-center gap-2 text-sm text-fg-muted">
              <Spinner size={14} /> {t("sim.loading")}
            </p>
          )}
          {state.phase === "unavailable" && (
            <p className="max-w-sm text-center text-sm text-fg-muted" data-testid="sim-unavailable">
              {state.message ?? t("sim.unavailable")}
            </p>
          )}
          {state.phase === "empty" && (
            <p className="max-w-sm text-center text-sm text-fg-muted" data-testid="sim-empty">
              {t("sim.empty")}
            </p>
          )}
          {state.phase === "off" && device && (
            <p className="text-sm text-fg-muted">{t("sim.off", { name: device.name })}</p>
          )}
          {(state.phase === "booting" || state.phase === "stopping") && device && (
            <p className="flex items-center gap-2 text-sm text-fg-muted" data-testid="sim-busy">
              <Spinner size={14} />
              {state.phase === "booting" ? t("sim.booting", { name: device.name }) : t("sim.stopping", { name: device.name })}
            </p>
          )}
          {state.phase === "failed" && (
            <div className="flex flex-col items-center gap-2 text-center" data-testid="sim-failed">
              <p role="alert" className="max-w-sm text-sm text-danger">
                {state.message}
              </p>
              <Button size="sm" variant="secondary" onClick={() => void refresh()}>
                {t("sim.retry")}
              </Button>
            </div>
          )}
          {state.phase === "live" &&
            (frame ? (
              <img
                ref={imgRef}
                src={frame}
                alt={t("sim.screen", { name: device?.name ?? "" })}
                data-testid="sim-frame"
                tabIndex={0}
                onClick={onClick}
                onKeyDown={onKeyDown}
                className="max-h-full max-w-full select-none rounded-md object-contain focus-visible:shadow-focus focus-visible:outline-none"
                draggable={false}
              />
            ) : (
              <p className="flex items-center gap-2 text-sm text-fg-muted">
                <Spinner size={14} /> {t("sim.waitingFrame")}
              </p>
            ))}
        </div>
        {state.phase === "live" && !input && (
          <p className="border-t border-divider px-2 py-1 text-xs text-fg-muted" data-testid="sim-view-only">
            {t("sim.viewOnly")}
          </p>
        )}
      </div>
      <DropZones paneId={pane.id} tabId={tabId} />
    </section>
  );
});

function Toolbar({
  status,
  udid,
  phase,
  onSelect,
  onBoot,
  onShutdown,
  onHome,
  onInstall,
}: {
  status: SimStatus;
  udid: string | null;
  phase: string;
  onSelect: (udid: string) => void;
  onBoot: () => void;
  onShutdown: () => void;
  onHome: () => void;
  onInstall: () => void;
}) {
  const busy = phase === "booting" || phase === "stopping";
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-divider bg-surface-2 px-2 py-1" data-testid="sim-toolbar">
      <Select
        ariaLabel={t("sim.device")}
        size="sm"
        className="min-w-48"
        value={udid ?? ""}
        disabled={busy}
        onValueChange={onSelect}
        options={status.devices.map((d) => ({
          value: d.udid,
          label: d.state === "booted" ? t("sim.deviceBooted", { name: d.name }) : d.name,
          group: d.runtime,
        }))}
      />
      {phase === "live" ? (
        <>
          <Button size="sm" variant="secondary" onClick={onShutdown} data-testid="sim-shutdown">
            {t("sim.shutdown")}
          </Button>
          {status.input && (
            <Button size="sm" variant="ghost" onClick={onHome}>
              {t("sim.home")}
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={onInstall}>
            {t("sim.install")}
          </Button>
        </>
      ) : (
        <Button size="sm" variant="primary" onClick={onBoot} disabled={busy || phase === "unavailable"} loading={phase === "booting"} data-testid="sim-boot">
          {t("sim.boot")}
        </Button>
      )}
    </div>
  );
}
