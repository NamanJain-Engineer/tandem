import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { cx } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { base64ToBytes, fontStack, type TerminalOptions } from "./TerminalView";

/** The last run's output, read-only (ARCHITECTURE §5.6: dimmed under the
 * card, with a separator). */
export function RestoredScrollback({
  paneId,
  options,
  dim,
}: {
  paneId: string;
  options: TerminalOptions;
  dim: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let disposed = false;
    const xterm = new Terminal({
      theme: options.theme,
      fontFamily: fontStack(options.fontFamily),
      fontSize: options.fontSize,
      lineHeight: 1.25,
      scrollback: options.scrollback,
      disableStdin: true,
      cursorBlink: false,
      cursorInactiveStyle: "none",
    });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.open(el);
    const refit = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) fit.fit();
    };
    refit();
    const observer = new ResizeObserver(refit);
    observer.observe(el);
    void invoke<string>("restore_scrollback", { paneId })
      .then((b64) => {
        if (disposed) return;
        xterm.write(base64ToBytes(b64));
        xterm.write(`\r\n\x1b[2m${t("restore.separator")}\x1b[0m\r\n`);
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      observer.disconnect();
      xterm.dispose();
    };
    // The snapshot never changes while the card shows.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paneId]);
  return (
    <div
      ref={host}
      data-testid="restored-scrollback"
      className={cx("absolute inset-0 overflow-hidden px-1 pt-1", dim && "opacity-40")}
      aria-label={t("restore.savedOutput")}
    />
  );
}
