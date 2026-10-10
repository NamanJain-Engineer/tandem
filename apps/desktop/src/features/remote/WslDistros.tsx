import { useEffect, useState } from "react";
import type { Distro, IpcError } from "@tandem/protocol";
import { Badge, Button, Callout, Dialog, EmptyState, Input, ListRow, RadioGroup, Skeleton, useToast } from "@tandem/ui";
import { Folder, Terminal } from "lucide-react";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSetting, useSettings } from "../../stores/settings";
import { useWorkspaces } from "../../stores/workspaces";
import { useUi } from "../../stores/ui";
import { useSession } from "../../stores/session";

type Load = { state: "loading" } | { state: "hidden" } | { state: "error"; message: string } | { state: "ready"; distros: Distro[] };

/** UI-111's WSL section (F-RM-02): the distros `wsl.exe -l -v` lists, the
 * one plain terminals open in, and "Open WSL folder". Windows only: on any
 * other system `wsl_distros` says so and the section isn't shown. */
export function WslDistros() {
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [opening, setOpening] = useState<string | null>(null);
  const plain = useSetting<string | null>("wsl_default_distro", null);
  const { show } = useToast();
  const refresh = async () => {
    setLoad({ state: "loading" });
    try {
      setLoad({ state: "ready", distros: await invoke<Distro[]>("wsl_distros", {}, { timeoutMs: 30_000 }) });
    } catch (e) {
      const err = e as IpcError;
      setLoad(err.stage === "wsl" && err.code === "unavailable" ? { state: "hidden" } : { state: "error", message: err.message });
    }
  };
  useEffect(() => {
    void refresh();
  }, []);
  if (load.state === "hidden") return null;
  const choose = async (value: string) => {
    const ok = await useSettings.getState().set("wsl_default_distro", value === "" ? null : value);
    if (!ok) show({ tone: "error", title: t("remote.wsl.saveFailed") });
  };
  return (
    <section className="flex flex-col gap-3" data-testid="wsl-distros">
      <h3 className="text-md font-semibold text-fg">{t("remote.wsl.title")}</h3>
      {load.state === "loading" ? (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-10" />
        </div>
      ) : load.state === "error" ? (
        <Callout
          tone="error"
          actions={
            <Button size="sm" variant="secondary" onClick={() => void refresh()}>
              {t("common.retry")}
            </Button>
          }
        >
          {load.message}
        </Callout>
      ) : load.distros.length === 0 ? (
        <EmptyState icon={<Terminal size={20} />} title={t("remote.wsl.empty.title")} body={t("remote.wsl.empty.body")} />
      ) : (
        <>
          <ul className="flex flex-col gap-1">
            {load.distros.map((d) => (
              <li key={d.name} data-testid="wsl-distro" data-name={d.name}>
                <ListRow
                  leading={<Terminal size={16} aria-hidden="true" className="text-fg-muted" />}
                  title={d.name}
                  secondary={
                    <span className="flex items-center gap-2">
                      {d.version !== null && <span>{t("remote.wsl.version", { n: d.version })}</span>}
                      {d.isDefault && <Badge variant="neutral">{t("remote.wsl.default")}</Badge>}
                    </span>
                  }
                  trailing={
                    <Button size="sm" variant="secondary" iconLeft={<Folder size={14} />} onClick={() => setOpening(d.name)}>
                      {t("remote.wsl.openFolder")}
                    </Button>
                  }
                />
              </li>
            ))}
          </ul>
          <RadioGroup
            label={t("remote.wsl.plain")}
            value={plain && load.distros.some((d) => d.name === plain) ? plain : ""}
            onValueChange={(v) => void choose(v)}
            items={[
              { value: "", label: t("remote.wsl.plainWindows") },
              ...load.distros.map((d) => ({ value: d.name, label: t("remote.wsl.plainIn", { distro: d.name }) })),
            ]}
          />
        </>
      )}
      <WslFolderDialog distro={opening} onClose={() => setOpening(null)} />
    </section>
  );
}

/** "Open WSL folder": a Linux path in the distro (a Windows one is
 * translated); the core checks it exists there before adding it. */
function WslFolderDialog({ distro, onClose }: { distro: string | null; onClose: () => void }) {
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  useEffect(() => {
    if (distro) {
      setPath("");
      setError(null);
    }
  }, [distro]);
  const open = async () => {
    if (!distro) return;
    setAdding(true);
    setError(null);
    const err = await useWorkspaces.getState().addRemote({ kind: "wsl", root: path.trim(), wslDistro: distro });
    setAdding(false);
    if (err) {
      if (err.code === "limit_reached")
        useUi.getState().openUpgrade({ kind: "workspaces", limit: useSession.getState().entitlement?.limits.workspaces ?? 3 });
      else setError(err.message);
      return;
    }
    onClose();
    useUi.getState().closeSettings();
    useUi.getState().setMode("code");
  };
  return (
    <Dialog
      open={distro !== null}
      onOpenChange={(o) => !o && onClose()}
      title={t("remote.wsl.folder.title", { distro: distro ?? "" })}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" data-testid="wsl-folder-open" disabled={!path.trim() || adding} loading={adding} onClick={() => void open()}>
            {t("remote.picker.open")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim()) void open();
        }}
      >
        <Input
          className="font-mono"
          label={t("remote.picker.path")}
          data-testid="wsl-folder-path"
          placeholder={t("remote.wsl.folder.placeholder")}
          hint={t("remote.wsl.folder.hint")}
          value={path}
          onChange={(e) => setPath(e.target.value)}
          spellCheck={false}
        />
        {error && <Callout tone="error">{error}</Callout>}
      </form>
    </Dialog>
  );
}
