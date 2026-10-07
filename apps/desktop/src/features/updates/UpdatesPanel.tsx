import { Button, ProgressBar, SegmentedControl } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUpdates } from "../../stores/updates";
import { useBoot } from "../shell/useBoot";

const DOWNLOAD_PAGE = "https://vibezeo.com/download/";

/** UI-115 Settings › Updates: version, channel, Check now and its stages. */
export function UpdatesPanel() {
  const state = useUpdates((s) => s.state);
  const boot = useBoot((s) => s.boot);
  const version = t("settings.updates.versionValue", { version: boot?.version ?? state?.current ?? "", build: boot?.build ?? "" });
  if (state?.stage === "managed")
    return (
      <div className="flex flex-col gap-2 text-sm text-fg" data-testid="updates-panel" data-stage="managed">
        <p>{version}</p>
        <p>
          {t("settings.updates.managed")}{" "}
          <Button variant="link" onClick={() => invoke("app_open_external", { url: DOWNLOAD_PAGE }).catch(() => undefined)}>
            {t("settings.updates.downloadPage")}
          </Button>
        </p>
      </div>
    );
  const stage = state?.stage ?? "idle";
  const line = (() => {
    if (!state) return null;
    switch (state.stage) {
      case "checking":
        return t("settings.updates.checking");
      case "downloading":
        return t("settings.updates.downloading", { version: state.version ?? "", percent: state.percent ?? 0 });
      case "ready":
        return t("settings.updates.ready", { version: state.version ?? "" });
      case "unavailable":
        return t("settings.updates.unavailable");
      case "error":
        if (state.errorStage === "verify") return t("settings.updates.cantVerify");
        if (state.errorStage === "check") return t("settings.updates.checkFailed");
        return t("settings.updates.failed", { stage: t(`update.stage.${state.errorStage ?? "download"}`) });
      default:
        return state.upToDate ? t("settings.updates.upToDate") : null;
    }
  })();
  return (
    <div className="flex flex-col gap-3 text-sm text-fg" data-testid="updates-panel" data-stage={stage}>
      <p data-testid="updates-version">{version}</p>
      <div className="flex items-center gap-3">
        <span className="text-fg-muted">{t("settings.updateChannel")}</span>
        <SegmentedControl
          label={t("settings.updateChannel")}
          size="sm"
          value={state?.channel ?? "stable"}
          onValueChange={(v) => v && void useUpdates.getState().setChannel(v as "stable" | "beta")}
          items={[
            { id: "stable", label: t("settings.updateChannel.stable") },
            { id: "beta", label: t("settings.updateChannel.beta") },
          ]}
        />
      </div>
      <div className="flex items-center gap-3">
        <Button
          size="sm"
          data-testid="updates-check-now"
          disabled={stage === "checking" || stage === "downloading" || stage === "unavailable"}
          onClick={() => void useUpdates.getState().checkNow()}
        >
          {t("settings.updates.checkNow")}
        </Button>
        {stage === "ready" && (
          <Button size="sm" variant="primary" onClick={() => useUpdates.getState().setRestartOpen(true)}>
            {t("update.restart")}
          </Button>
        )}
        {line && (
          <span role="status" data-testid="updates-status" className={stage === "error" ? "text-danger" : "text-fg-muted"}>
            {line}
          </span>
        )}
      </div>
      {stage === "downloading" && <ProgressBar value={(state?.percent ?? 0) / 100} label={t("settings.updates.checkNow")} />}
    </div>
  );
}
