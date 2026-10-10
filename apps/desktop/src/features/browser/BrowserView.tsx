// The browser (F-CD-17): a toolbar and the place a page shows in. The page
// is a webview the core draws over the place (D-206) and keeps while its tab
// or pane exists, so a view that comes and goes finds it again by id. The
// address bar shows what the core reports as committed, never what a page
// says about itself.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, ArrowRight, Crosshair, Ellipsis, Globe, Monitor, RotateCw, Smartphone, Tablet, X } from "lucide-react";
import type { BrowserCmd, BrowserState, Device, Rect } from "@tandem/protocol";
import { Banner, Button, ConfirmDialog, EmptyState, IconButton, Input, Menu, Spinner, useToast, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { invoke } from "../../ipc/invoke";
import { takeAddressFocus, useBrowser } from "../../stores/browser";
import { PickerPanel } from "./PickerPanel";
import { notePage, usePlace, ZERO, type Placed } from "./place";

const DEVICE_ICON: Record<Device, ReactNode> = {
  desktop: <Monitor size={14} />,
  phone: <Smartphone size={14} />,
  tablet: <Tablet size={14} />,
};

/** CSS sizes (ARCHITECTURE §3.2 `browser_set_device`). */
const SIZES = { phone: [390, 844], tablet: [820, 1180] } as const;

/** Room above a device frame for its size label (the core's fit). */
const LABEL = 24;

/** Addresses the system browser opens. */
const isWeb = (url: string | null | undefined): url is string => !!url && /^https?:\/\//i.test(url);

/** What the system browser gets for typed text: an address as it is, a
 * bare host over http (the core's rule), anything else nothing. */
export function externalFor(text: string): string | null {
  const value = text.trim();
  if (isWeb(value)) return value;
  if (/^(localhost|[\w-]+(\.[\w-]+)+)(:\d+)?([/?#]\S*)?$/i.test(value)) return `http://${value}`;
  return null;
}

/** A page with nothing to show stays hidden, so the empty state shows. */
const hasPage = (s: BrowserState | undefined): s is BrowserState => !!s && (s.url !== "" || s.loading);

const openExternal = (url: string) => void invoke("app_open_external", { url }).catch(() => undefined);

/** The device preview's outline and its CSS size above it. */
function DeviceFrame({ state, bounds }: { state: BrowserState; bounds: Rect }) {
  if (state.device === "desktop") return null;
  const [w, h] = SIZES[state.device];
  const [cw, ch] = state.rotated ? [h, w] : [w, h];
  const zoom = Math.round(state.zoom * 100);
  const size = { w: String(cw), h: String(ch) };
  const f = state.frame;
  return (
    <>
      <div
        data-testid="browser-device-label"
        className="pointer-events-none absolute inset-x-0 top-0 flex items-center justify-center text-2xs tabular-nums text-fg-muted"
        style={{ height: LABEL }}
      >
        {zoom < 100 ? t("browser.deviceZoom", { ...size, zoom: String(zoom) }) : t("browser.deviceSize", size)}
      </div>
      <div
        aria-hidden="true"
        className="pointer-events-none absolute rounded-xs ring-1 ring-border-strong"
        style={{ left: f.x - bounds.x, top: f.y - bounds.y, width: f.width, height: f.height }}
      />
    </>
  );
}

export type BrowserViewProps = {
  /** The sidebar tab's or the pane's id: the page's id in the core. */
  id: string;
  /** The page to reopen (a restored tab), if any. */
  url: string | null;
  /** Hide the page while the app shows something over its place (a pane
   * card, a pane being dragged). */
  suspended?: boolean;
  /** More ⋯ items: moving the page between a pane and the sidebar. */
  actions?: MenuEntry[];
};

export function BrowserView({ id, url, suspended = false, actions = [] }: BrowserViewProps) {
  const page = useBrowser((s) => s.pages[id]);
  const unavailable = useBrowser((s) => s.unavailable[id]);
  const pickedCount = useBrowser((s) => s.picked[id]?.length ?? 0);
  const toast = useToast();
  const place = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const placed = useRef<Placed>({ bounds: ZERO, shown: false });
  const opening = useRef<Promise<boolean> | null>(null);
  const failed = useRef<string | null>(null);
  const restoreUrl = useRef(url);
  restoreUrl.current = url;
  const held = useRef(suspended);
  held.current = suspended;
  const [bounds, setBounds] = useState<Rect>(ZERO);
  const [visible, setVisible] = useState(false);
  /** What's in the field while it's edited. */
  const [draft, setDraft] = useState<string | null>(null);
  /** What was asked for, until the page commits (or doesn't). */
  const [pending, setPending] = useState<string | null>(null);
  const [typed, setTyped] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);

  /** Tells the core where the page goes and whether it shows. */
  const sync = () => {
    const { bounds, shown } = placed.current;
    const store = useBrowser.getState();
    const state = store.pages[id];
    if (state) {
      const show = shown && !held.current && hasPage(state);
      store.place(id, bounds, show);
      notePage(id, show ? bounds : null);
      setVisible(show);
      return;
    }
    // A restored tab opens its page the first time it shows.
    const at = restoreUrl.current;
    if (shown && at && !opening.current && !store.unavailable[id] && !failed.current) void ensureOpen(at);
  };

  const ensureOpen = (at: string | null): Promise<boolean> => {
    opening.current ??= (async () => {
      const { bounds, shown } = placed.current;
      const err = await useBrowser.getState().open(id, at, bounds, shown && !held.current && at !== null);
      if (err) {
        opening.current = null;
        if (err.code !== "unavailable") {
          failed.current = err.message;
          setError(err.message);
        }
        return false;
      }
      failed.current = null;
      setError(null);
      sync();
      return true;
    })();
    return opening.current;
  };

  usePlace(place, (p) => {
    placed.current = p;
    setBounds(p.bounds);
    sync();
  });

  // A page shows once it has something to show.
  const latestSync = useRef(sync);
  latestSync.current = sync;
  const showable = hasPage(page);
  useEffect(() => latestSync.current(), [showable, suspended]);

  useEffect(() => {
    if (!takeAddressFocus(id)) return;
    const frame = requestAnimationFrame(() => field.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [id]);

  // Gone (closed, or moved to a pane or the sidebar): the page hides until
  // its next place reports.
  useEffect(
    () => () => {
      if (useBrowser.getState().pages[id]) useBrowser.getState().place(id, placed.current.bounds, false);
      notePage(id, null);
    },
    [id],
  );

  // What was asked for shows until the page commits, refuses, or stops.
  const before = useRef(page);
  useEffect(() => {
    const was = before.current;
    before.current = page;
    if (!page || !was) return;
    if (page.url !== was.url || (page.blocked !== null && page.blocked !== was.blocked) || (was.loading && !page.loading)) setPending(null);
  }, [page]);

  const go = async (text: string) => {
    const input = text.trim();
    if (!input) return;
    setDraft(null);
    setTyped(input);
    setDismissed(null);
    if (!useBrowser.getState().pages[id] && !(await ensureOpen(null))) return;
    setPending(input);
    const err = await useBrowser.getState().navigate(id, input);
    if (err) setPending(null);
  };

  const command = (cmd: BrowserCmd) => useBrowser.getState().command(id, cmd);
  const setDevice = (device: Device, rotated: boolean) => useBrowser.getState().setDevice(id, device, rotated);

  const committed = page?.url || url || "";
  const value = draft ?? pending ?? committed;
  const loading = page?.loading ?? false;
  const device = page?.device ?? "desktop";
  const rotated = page?.rotated ?? false;
  const blocked = page?.blocked ?? null;
  const picking = page?.picking ?? false;
  const target = isWeb(url) ? url : typed ? externalFor(typed) : null;

  const devices: MenuEntry[] = [
    { type: "checkbox", id: "desktop", label: t("browser.device.desktop"), checked: device === "desktop", onCheckedChange: () => setDevice("desktop", false) },
    { type: "checkbox", id: "phone", label: t("browser.device.phone"), checked: device === "phone", onCheckedChange: () => setDevice("phone", rotated) },
    { type: "checkbox", id: "tablet", label: t("browser.device.tablet"), checked: device === "tablet", onCheckedChange: () => setDevice("tablet", rotated) },
    { type: "separator", id: "sep" },
    {
      type: "checkbox",
      id: "rotate",
      label: t("browser.rotate"),
      checked: rotated,
      disabled: device === "desktop",
      onCheckedChange: (on) => setDevice(device, on),
    },
  ];
  const more: MenuEntry[] = [
    { id: "external", label: t("browser.openExternal"), disabled: !isWeb(committed), onSelect: () => openExternal(committed) },
    { id: "copy", label: t("browser.copyAddress"), disabled: !committed, onSelect: () => void copyText(committed) },
    ...(actions.length > 0 ? [{ type: "separator" as const, id: "sep-move" }, ...actions] : []),
    { id: "clear", label: t("browser.clear"), danger: true, onSelect: () => setClearing(true) },
  ];

  return (
    <div className="flex h-full flex-col" data-testid="browser-view" data-browser-id={id}>
      <div className="flex h-9 shrink-0 items-center gap-1 border-b border-divider px-2" data-testid="browser-toolbar">
        {/* Tooltips go up: below, the page would cover them. */}
        <IconButton size="sm" tooltipSide="top" icon={<ArrowLeft size={14} />} label={t("browser.back")} disabled={!page?.canBack} onClick={() => command("back")} />
        <IconButton
          size="sm"
          tooltipSide="top"
          icon={<ArrowRight size={14} />}
          label={t("browser.forward")}
          disabled={!page?.canForward}
          onClick={() => command("forward")}
        />
        {loading ? (
          <IconButton size="sm" tooltipSide="top" icon={<X size={14} />} label={t("browser.stop")} onClick={() => command("stop")} />
        ) : (
          <IconButton size="sm" tooltipSide="top" icon={<RotateCw size={14} />} label={t("browser.reload")} disabled={!showable} onClick={() => command("reload")} />
        )}
        <Input
          ref={field}
          hideLabel
          label={t("browser.address")}
          placeholder={t("browser.addressPlaceholder")}
          data-testid="browser-address"
          className="min-w-0 flex-1"
          value={value}
          prefix={loading ? <Spinner size={12} /> : <Globe size={12} aria-hidden="true" />}
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="off"
          onFocus={(e) => {
            setDraft(e.currentTarget.value);
            e.currentTarget.select();
          }}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => setDraft(null)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void go(e.currentTarget.value);
            } else if (e.key === "Escape" && draft !== null && draft !== committed) {
              // Back to the page's address; a second Esc goes on (full-screen).
              e.preventDefault();
              setDraft(committed);
              const input = e.currentTarget;
              requestAnimationFrame(() => input.select());
            }
          }}
        />
        <IconButton
          size="sm"
          tooltipSide="top"
          icon={<Crosshair size={14} />}
          label={picking ? t("browser.pick.stop") : t("browser.pick.start")}
          pressed={picking}
          disabled={!showable}
          data-testid="browser-pick"
          onClick={() => void useBrowser.getState().pick(id, !picking)}
        />
        <Menu
          align="end"
          trigger={<IconButton size="sm" tooltipSide="top" icon={DEVICE_ICON[device]} label={t("browser.device")} disabled={!page} data-testid="browser-device" />}
          entries={devices}
        />
        <Menu align="end" trigger={<IconButton size="sm" tooltipSide="top" icon={<Ellipsis size={14} />} label={t("browser.more")} data-testid="browser-more" />} entries={more} />
      </div>
      {blocked !== null && blocked !== dismissed && (
        <Banner tone="warning" onDismiss={() => setDismissed(blocked)} dismissLabel={t("common.dismiss")}>
          <span data-testid="browser-blocked">{t("browser.blocked", { url: blocked })}</span>
        </Banner>
      )}
      {error && <Banner tone="error">{error}</Banner>}
      {(picking || pickedCount > 0) && <PickerPanel id={id} picking={picking} />}
      <div ref={place} className="relative min-h-0 flex-1" data-testid="browser-place" data-visible={visible}>
        {page && showable && <DeviceFrame state={page} bounds={bounds} />}
        {!showable &&
          (unavailable ? (
            <EmptyState
              icon={<Globe size={20} />}
              title={t("browser.unavailable.title")}
              body={unavailable}
              primary={target ? <Button onClick={() => openExternal(target)}>{t("browser.openInBrowser")}</Button> : undefined}
            />
          ) : (
            <EmptyState icon={<Globe size={20} />} title={t("browser.empty.title")} body={t("browser.empty.body")} />
          ))}
      </div>
      <ConfirmDialog
        open={clearing}
        onOpenChange={setClearing}
        title={t("browser.clear.title")}
        body={t("browser.clear.body")}
        confirmLabel={t("browser.clear.confirm")}
        cancelLabel={t("common.cancel")}
        danger
        onConfirm={() =>
          void useBrowser
            .getState()
            .clearData()
            .then((err) => toast.show(err ? { tone: "error", title: t("browser.clear.failed"), body: err.message } : { title: t("browser.cleared") }))
        }
      />
    </div>
  );
}
