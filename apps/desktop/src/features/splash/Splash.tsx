import { t } from "../../i18n";
import { LogoMark } from "../shell/LogoMark";

export type SplashProps = {
  /** Restore progress; omitted while the app is still starting. */
  progress?: { done: number; total: number };
  onSkip?: () => void;
};

/** UI-02 launch / restore splash. */
export function Splash({ progress, onSkip }: SplashProps) {
  const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
  return (
    <main className="flex h-full items-center justify-center bg-bg" aria-busy="true">
      <section className="flex w-80 flex-col items-center gap-4 text-center">
        <div className="flex items-center gap-3">
          <LogoMark />
          <h1 className="text-2xl font-semibold tracking-tight text-fg">{t("app.name")}</h1>
        </div>
        <p className="text-base text-fg-muted" role="status" aria-live="polite">
          {progress ? t("splash.restoring") : t("splash.starting")}
        </p>
        {progress && (
          <>
            <div
              className="h-1.5 w-full overflow-hidden rounded-full bg-surface-4"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={progress.total}
              aria-valuenow={progress.done}
              aria-label={t("splash.restoring")}
            >
              <div
                className="h-full bg-accent transition-[width] duration-[var(--dur-base)]"
                style={{ width: `${pct}%` }}
              />
            </div>
            <p className="text-sm text-fg-subtle tabular-nums">
              {t("splash.progress", { n: progress.done, m: progress.total })}
            </p>
            {onSkip && (
              <button type="button" className="text-sm text-accent hover:underline" onClick={onSkip}>
                {t("splash.skip")}
              </button>
            )}
          </>
        )}
      </section>
    </main>
  );
}

/** Tandem's mark: two overlapping rings. */
