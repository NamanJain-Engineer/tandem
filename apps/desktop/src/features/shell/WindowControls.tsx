import { useEffect, useState } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import type { WindowMaximized } from "@tandem/protocol";
import { cx } from "@tandem/ui";
import { t } from "../../i18n";
import { inTauri, invoke, usingFakeBackend } from "../../ipc/invoke";

const BTN =
  "inline-flex h-10 w-[46px] items-center justify-center text-fg-muted transition-colors duration-[var(--dur-instant)] hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none";

/** Custom min / max-restore / close for Windows and Linux (F-SH-01 item 1). */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!inTauri() || usingFakeBackend()) return;
    let off: (() => void) | undefined;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const w = getCurrentWindow();
      setMaximized(await w.isMaximized());
      off = await w.onResized(async () => setMaximized(await w.isMaximized()));
    })();
    return () => off?.();
  }, []);
  const label = maximized ? t("titlebar.restore") : t("titlebar.maximize");
  return (
    <div className="flex items-center self-stretch">
      <button
        type="button"
        aria-label={t("titlebar.minimize")}
        title={t("titlebar.minimize")}
        className={BTN}
        onClick={() => invoke("window_minimize").catch(() => undefined)}
      >
        <Minus size={16} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label={label}
        title={label}
        data-testid="window-maximize"
        className={BTN}
        onClick={() =>
          invoke<WindowMaximized>("window_toggle_maximize")
            .then((r) => setMaximized(r.maximized))
            .catch(() => undefined)
        }
      >
        {maximized ? <Copy size={14} aria-hidden="true" /> : <Square size={13} aria-hidden="true" />}
      </button>
      <button
        type="button"
        aria-label={t("titlebar.close")}
        title={t("titlebar.close")}
        className={cx(BTN, "hover:bg-danger hover:text-danger-fg")}
        onClick={() => invoke("window_close").catch(() => undefined)}
      >
        <X size={16} aria-hidden="true" />
      </button>
    </div>
  );
}
