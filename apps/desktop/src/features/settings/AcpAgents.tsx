import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import type { AcpAgent, AcpAgentInput, AgentInfo, IpcError } from "@tandem/protocol";
import { Banner, Button, ConfirmDialog, Dialog, IconButton, Input, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { parseArgs } from "./EnginesPage";

/** The core waits 10 s for an agent's answer; the window waits longer, so
 * the core's own "Not an ACP agent" is what the user sees. */
const PROBE_MS = 30_000;

/** Goose documents `goose acp` for clients like this one. */
const GOOSE: AcpAgentInput = { label: "Goose", command: "goose", args: ["acp"], env: {} };

type Row = { key: string; value: string };

/** `NAME=value` rows to a map; `null` when a row has a name and no `=`. */
export function envOf(rows: Row[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) if (r.key.trim()) out[r.key.trim()] = r.value;
  return out;
}

function AddAgent({ onClose, onAdded }: { onClose: () => void; onAdded: (a: AcpAgent) => void }) {
  const [label, setLabel] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [env, setEnv] = useState<Row[]>([]);
  const [busy, setBusy] = useState<"test" | "add" | null>(null);
  const [tested, setTested] = useState<AgentInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const parsed = parseArgs(args);
  const input = (): AcpAgentInput | null =>
    parsed === null ? null : { label: label.trim(), command: command.trim(), args: parsed, env: envOf(env) };
  const ready = label.trim() !== "" && command.trim() !== "" && parsed !== null && busy === null;
  const changed = () => {
    setTested(null);
    setError(null);
  };
  const run = async (what: "test" | "add") => {
    const value = input();
    if (!value) return;
    setBusy(what);
    setError(null);
    try {
      if (what === "test")
        setTested(await invoke<AgentInfo>("acp_agent_test", { input: value }, { timeoutMs: PROBE_MS }));
      else onAdded(await invoke<AcpAgent>("acp_agent_add", { input: value }, { timeoutMs: PROBE_MS }));
    } catch (e) {
      setTested(null);
      setError((e as IpcError).message);
    } finally {
      setBusy(null);
    }
  };
  const preset = (p: AcpAgentInput) => {
    setLabel(p.label);
    setCommand(p.command);
    setArgs(p.args.join(" "));
    setEnv([]);
    changed();
  };
  return (
    <Dialog
      open
      size="md"
      onOpenChange={(o) => !o && onClose()}
      title={t("acp.addTitle")}
      description={t("acp.addBody")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="secondary"
            disabled={!ready}
            loading={busy === "test"}
            onClick={() => void run("test")}
            data-testid="acp-test"
          >
            {t("acp.test")}
          </Button>
          <Button
            variant="primary"
            disabled={!ready}
            loading={busy === "add"}
            onClick={() => void run("add")}
            data-testid="acp-add-confirm"
          >
            {t("acp.add")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="acp-add">
        <div className="flex items-center gap-2 text-sm text-fg-muted">
          <span>{t("acp.presets")}</span>
          <Button size="sm" variant="ghost" onClick={() => preset(GOOSE)} data-testid="acp-preset-goose">
            {GOOSE.label}
          </Button>
        </div>
        <Input
          label={t("acp.label")}
          value={label}
          maxLength={40}
          onChange={(e) => {
            setLabel(e.target.value);
            changed();
          }}
          data-testid="acp-label"
        />
        <Input
          label={t("acp.command")}
          hint={t("acp.commandHint")}
          value={command}
          monospace
          onChange={(e) => {
            setCommand(e.target.value);
            changed();
          }}
          data-testid="acp-command"
        />
        <Input
          label={t("acp.args")}
          value={args}
          monospace
          error={parsed === null ? t("engines.args.unbalanced") : undefined}
          onChange={(e) => {
            setArgs(e.target.value);
            changed();
          }}
          data-testid="acp-args"
        />
        <fieldset className="flex flex-col gap-1.5">
          <legend className="text-sm text-fg">{t("acp.env")}</legend>
          <p className="text-xs text-fg-subtle">{t("acp.envHint")}</p>
          {env.map((row, i) => (
            <div key={i} className="flex items-end gap-2">
              <Input
                label={t("acp.envName")}
                hideLabel
                placeholder="NAME"
                monospace
                value={row.key}
                onChange={(e) => {
                  setEnv(env.map((r, j) => (j === i ? { ...r, key: e.target.value } : r)));
                  changed();
                }}
              />
              <Input
                label={t("acp.envValue")}
                hideLabel
                placeholder="value"
                monospace
                value={row.value}
                onChange={(e) => {
                  setEnv(env.map((r, j) => (j === i ? { ...r, value: e.target.value } : r)));
                  changed();
                }}
              />
              <IconButton
                label={t("acp.envRemove")}
                icon={<Trash2 size={14} />}
                onClick={() => {
                  setEnv(env.filter((_, j) => j !== i));
                  changed();
                }}
              />
            </div>
          ))}
          <div>
            <Button size="sm" variant="ghost" onClick={() => setEnv([...env, { key: "", value: "" }])}>
              <Plus size={14} aria-hidden="true" />
              {t("acp.envAdd")}
            </Button>
          </div>
        </fieldset>
        {tested && (
          <Banner tone="success">
            <span data-testid="acp-tested">
              {t("acp.tested", {
                name: tested.name ?? label,
                version: tested.version ?? t("acp.noVersion"),
              })}
              {tested.authMethods.length > 0 &&
                ` ${t("acp.signIn", { methods: tested.authMethods.join(", ") })}`}
            </span>
          </Banner>
        )}
        {error && (
          <Banner tone="error">
            <span data-testid="acp-error">{error}</span>
          </Banner>
        )}
      </div>
    </Dialog>
  );
}

/** UI-163 ACP agents (F-EN-10): the agents the user added, Add and Remove. */
export function AcpAgents() {
  const [agents, setAgents] = useState<AcpAgent[] | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<AcpAgent | null>(null);
  const { show } = useToast();
  const load = useCallback(() => {
    void invoke<AcpAgent[] | null>("acp_agents_list", {}).then(
      (a) => {
        setAgents(a ?? []);
        setFailed(null);
      },
      (e: IpcError) => setFailed(e.message),
    );
  }, []);
  useEffect(load, [load]);
  const changed = () => {
    load();
    void useEngines.getState().refresh();
  };
  return (
    <section className="flex flex-col gap-1.5" data-testid="acp-agents">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{t("acp.title")}</h3>
        <Button size="sm" variant="secondary" onClick={() => setAdding(true)} data-testid="acp-add-open">
          <Plus size={14} aria-hidden="true" />
          {t("acp.addOpen")}
        </Button>
      </div>
      {failed ? (
        <Banner tone="error" action={{ label: t("acp.retry"), onClick: load }}>
          {failed}
        </Banner>
      ) : agents === null ? (
        <Spinner />
      ) : agents.length === 0 ? (
        <p className="text-sm text-fg-muted" data-testid="acp-empty">
          {t("acp.empty")}
        </p>
      ) : (
        <ul className="rounded-md border border-border bg-surface-2">
          {agents.map((a) => (
            <li
              key={a.id}
              data-testid="acp-agent"
              data-id={a.id}
              className="flex items-center gap-2 border-b border-divider px-3 py-2 last:border-b-0"
            >
              <span className="w-40 truncate text-sm text-fg">{a.label}</span>
              <span
                className="min-w-0 flex-1 truncate font-mono text-xs text-fg-muted"
                title={[a.program, ...a.args].join(" ")}
              >
                {[a.program, ...a.args].join(" ")}
              </span>
              <Button size="sm" variant="ghost" onClick={() => setRemoving(a)}>
                {t("acp.remove")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {adding && (
        <AddAgent
          onClose={() => setAdding(false)}
          onAdded={(a) => {
            setAdding(false);
            show({ tone: "success", title: t("acp.added", { name: a.label }) });
            changed();
          }}
        />
      )}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(o) => !o && setRemoving(null)}
        danger
        title={t("acp.removeTitle", { name: removing?.label ?? "" })}
        body={t("acp.removeBody")}
        confirmLabel={t("acp.remove")}
        onConfirm={() => {
          const a = removing;
          setRemoving(null);
          if (a)
            void invoke("acp_agent_remove", { id: a.id }).then(changed, (e: IpcError) =>
              show({ tone: "error", title: e.message }),
            );
        }}
      />
    </section>
  );
}
