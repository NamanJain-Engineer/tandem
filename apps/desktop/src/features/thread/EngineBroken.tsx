import { useEffect, useState } from "react";
import type { EngineVersionView } from "@tandem/protocol";
import { Banner } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useWorkspaces } from "../../stores/workspaces";
import { useInstall } from "../code/InstallSheet";

/** F-EN-08: a failed thread whose engine changed since its last good
 * version says so, with Roll back (the install sheet, its command shown
 * first). Read on this computer only. */
export function EngineBroken({ engineId, failed }: { engineId: string; failed: boolean }) {
  const [view, setView] = useState<EngineVersionView | null>(null);
  const label = useEngines((s) => s.label(engineId)) ?? engineId;
  const tabId = useWorkspaces((s) => (s.activeId ? s.activeTab[s.activeId] : undefined));
  useEffect(() => {
    if (!failed) {
      setView(null);
      return;
    }
    let live = true;
    void invoke<EngineVersionView[]>("engine_versions", {}).then(
      (all) => live && setView(all.find((v) => v.engineId === engineId) ?? null),
      () => live && setView(null),
    );
    return () => {
      live = false;
    };
  }, [engineId, failed]);
  const broken = view?.broken;
  if (!failed || !broken) return null;
  const engine = useEngines.getState().list.find((e) => e.spec.id === engineId);
  return (
    <div data-testid="engine-broken">
      <Banner
        tone="warning"
        className="rounded-md"
        action={
          engine
            ? {
                label: t("engines.rollbackTo", { version: broken.from }),
                onClick: () => useInstall.getState().open(engine, tabId ?? null, broken.from),
              }
            : undefined
        }
      >
        {t("engines.broken", { name: label, from: broken.from, to: broken.to })}
      </Banner>
    </div>
  );
}
