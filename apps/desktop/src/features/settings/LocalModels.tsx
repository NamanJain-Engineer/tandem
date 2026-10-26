import { useCallback, useEffect, useState } from "react";
import { Plus, RefreshCw } from "lucide-react";
import type { IpcError, LibraryOpened, LocalModel, LocalServer, ServerKind } from "@tandem/protocol";
import { Banner, Button, Dialog, Input, Select, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { showOpened } from "../../stores/handoff";
import { useUi } from "../../stores/ui";
import { localTabId, useWorkspaces } from "../../stores/workspaces";

const KINDS: Array<{ value: ServerKind; label: string }> = [
  { value: "ollama", label: "Ollama" },
  { value: "lm_studio", label: "LM Studio" },
  { value: "llama_cpp", label: "llama.cpp" },
];
const kindLabel = (k: ServerKind) => KINDS.find((x) => x.value === k)?.label ?? k;

/** "18.6 GB", from bytes. */
export function gigabytes(bytes: number): string {
  return `${(bytes / 1e9).toFixed(1)} GB`;
}

/** "qwen3-coder:30b · 18.6 GB · 30.5B": what the server says, no more. */
export function modelLine(m: LocalModel): string {
  return [m.id, m.size ? gigabytes(m.size) : null, m.parameters].filter(Boolean).join(" · ");
}

function AddPort({ onClose, onAdded }: { onClose: () => void; onAdded: () => void }) {
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("");
  const [kind, setKind] = useState<ServerKind>("ollama");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const n = Number(port);
  const valid = Number.isInteger(n) && n >= 1 && n <= 65535;
  const add = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("local_models_add_port", { host, port: n, kind });
      onAdded();
    } catch (e) {
      setError((e as IpcError).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      size="sm"
      onOpenChange={(o) => !o && onClose()}
      title={t("local.addTitle")}
      description={t("local.addBody")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!valid || busy}
            loading={busy}
            onClick={() => void add()}
            data-testid="local-add-confirm"
          >
            {t("local.add")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="local-add">
        <Select
          label={t("local.kind")}
          value={kind}
          onValueChange={(v) => setKind(v as ServerKind)}
          options={KINDS}
        />
        <Input
          label={t("local.host")}
          value={host}
          monospace
          onChange={(e) => setHost(e.target.value)}
          data-testid="local-host"
        />
        <Input
          label={t("local.port")}
          value={port}
          inputMode="numeric"
          monospace
          error={port && !valid ? t("local.portRange") : undefined}
          onChange={(e) => setPort(e.target.value.trim())}
          data-testid="local-port"
        />
        {error && (
          <Banner tone="error">
            <span data-testid="local-error">{error}</span>
          </Banner>
        )}
      </div>
    </Dialog>
  );
}

function Server({ s, onChanged }: { s: LocalServer; onChanged: () => void }) {
  const tabId = useWorkspaces(localTabId);
  const codex = useEngines((st) => st.list.find((e) => e.spec.id === "codex"));
  const { show } = useToast();
  const [starting, setStarting] = useState<string | null>(null);
  const why = !s.startable
    ? t("local.notYet", { kind: kindLabel(s.kind) })
    : !codex?.install
      ? t("local.needsCodex")
      : !tabId
        ? t("local.needsFolder")
        : undefined;
  const start = async (model: string) => {
    if (!tabId) return;
    setStarting(model);
    try {
      const opened = await invoke<LibraryOpened>("local_model_start", {
        tabId,
        kind: s.kind,
        port: s.port,
        model,
      });
      show({ tone: "success", title: t("local.started", { model }) });
      useUi.getState().closeSettings();
      await showOpened(opened);
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    } finally {
      setStarting(null);
    }
  };
  return (
    <li
      data-testid="local-server"
      data-kind={s.kind}
      data-port={s.port}
      className="flex flex-col gap-1.5 px-3 py-2"
    >
      <div className="flex items-center gap-2">
        <span className="text-sm text-fg">{kindLabel(s.kind)}</span>
        <span className="font-mono text-xs text-fg-muted">{t("local.address", { port: s.port })}</span>
        <span className="flex-1 text-xs text-fg-subtle" data-testid="local-state">
          {s.running
            ? s.models.length === 1
              ? t("local.runningOne")
              : t("local.running", { n: s.models.length })
            : t("local.notRunning")}
        </span>
        {s.added && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() =>
              void invoke("local_models_remove_port", { port: s.port }).then(onChanged, (e: IpcError) =>
                show({ tone: "error", title: e.message }),
              )
            }
          >
            {t("local.removePort")}
          </Button>
        )}
      </div>
      {s.running && s.models.length > 0 && (
        <ul className="flex flex-col">
          {s.models.map((m, i) => (
            <li
              key={`${i}-${m.id}`}
              data-testid="local-model"
              data-model={m.id}
              className="flex items-center gap-2 py-1"
            >
              <span className="min-w-0 flex-1 truncate font-mono text-xs text-fg-muted" title={modelLine(m)}>
                {modelLine(m)}
              </span>
              <Button
                size="sm"
                variant="secondary"
                disabled={why !== undefined || starting !== null}
                title={why}
                loading={starting === m.id}
                onClick={() => void start(m.id)}
                data-testid="local-start"
              >
                {t("local.start")}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

/** UI-164 Local models (F-EN-11): model servers on this computer. */
export function LocalModels() {
  const [servers, setServers] = useState<LocalServer[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [adding, setAdding] = useState(false);
  const load = useCallback(() => {
    setChecking(true);
    void invoke<LocalServer[] | null>("local_models_list", {}).then(
      (s) => {
        setServers(s ?? []);
        setFailed(null);
        setChecking(false);
      },
      (e: IpcError) => {
        setFailed(e.message);
        setChecking(false);
      },
    );
  }, []);
  useEffect(load, [load]);
  const running = servers?.filter((s) => s.running || s.added) ?? [];
  return (
    <section className="flex flex-col gap-1.5" data-testid="local-models">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{t("local.title")}</h3>
        <div className="flex gap-2">
          <Button size="sm" variant="ghost" onClick={() => setAdding(true)} data-testid="local-add-open">
            <Plus size={14} aria-hidden="true" />
            {t("local.addOpen")}
          </Button>
          <Button size="sm" variant="secondary" disabled={checking} onClick={load} data-testid="local-check">
            {checking ? <Spinner /> : <RefreshCw size={14} aria-hidden="true" />}
            {t("local.check")}
          </Button>
        </div>
      </div>
      <p className="text-xs text-fg-subtle">{t("local.body")}</p>
      {failed ? (
        <Banner tone="error" action={{ label: t("local.retry"), onClick: load }}>
          {failed}
        </Banner>
      ) : servers === null ? (
        <Spinner />
      ) : running.length === 0 ? (
        <p className="text-sm text-fg-muted" data-testid="local-empty">
          {t("local.empty")}
        </p>
      ) : (
        <ul className="divide-y divide-divider rounded-md border border-border bg-surface-2">
          {running.map((s) => (
            <Server key={`${s.kind}-${s.port}`} s={s} onChanged={load} />
          ))}
        </ul>
      )}
      {adding && (
        <AddPort
          onClose={() => setAdding(false)}
          onAdded={() => {
            setAdding(false);
            load();
          }}
        />
      )}
    </section>
  );
}
