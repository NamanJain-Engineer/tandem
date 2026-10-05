import { useState } from "react";
import { EmptyState, Tabs } from "@tandem/ui";
import { t } from "../../i18n";

/** Right sidebar shell (F-SH-10 arrives in M4); tabs per UI §3.5. */
export function Sidebar() {
  const [tab, setTab] = useState("dashboard");
  return (
    <aside aria-label={t("sidebar.label")} className="flex w-[var(--sidebar-w)] shrink-0 flex-col border-l border-border bg-surface-1 px-3 pt-1">
      <Tabs
        label={t("sidebar.label")}
        value={tab}
        onValueChange={setTab}
        items={[
          { id: "dashboard", label: t("sidebar.dashboard") },
          { id: "browser", label: t("sidebar.browser") },
          { id: "files", label: t("sidebar.files") },
          { id: "tasks", label: t("sidebar.tasks") },
        ]}
        renderPanel={() => <EmptyState title={t("sidebar.soon")} />}
      />
    </aside>
  );
}
