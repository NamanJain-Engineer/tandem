import { useCallback, useEffect, useState, type ReactNode } from "react";
import { ChevronLeft, Lock, MoreHorizontal } from "lucide-react";
import type { IpcError, PluginCard, PluginDetail, PluginOptions, RiskClass } from "@tandem/protocol";
import {
  Badge,
  Banner,
  ConfirmDialog,
  EngineGlyph,
  IconButton,
  KeyValue,
  Menu,
  Skeleton,
  Switch,
  Tabs,
  useToast,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { listen } from "../../ipc/events";
import { riskLabel, riskVariant } from "./model";

/** How each class is decided (ARCHITECTURE §11.7), for UI-93's rows. */
const POLICY: Array<{ risk: RiskClass; locked: boolean }> = [
  { risk: "read", locked: false },
  { risk: "write", locked: false },
  { risk: "external_publish", locked: false },
  { risk: "credit_consuming", locked: true },
  { risk: "spend", locked: true },
  { risk: "destructive", locked: true },
  { risk: "unclassified", locked: true },
];

function policyText(risk: RiskClass): string {
  switch (risk) {
    case "read":
      return t("plugins.policy.run");
    case "write":
    case "external_publish":
      return t("plugins.policy.oncePerChat");
    default:
      return t("plugins.policy.everyTime");
  }
}

/** UI-92: one connected plugin, with its teammates, approval policy (UI-93)
 * and tools. */
export function PluginDetailView({
  connectionId,
  onBack,
  onReconnect,
  connectSheet,
}: {
  connectionId: string;
  onBack: () => void;
  onReconnect: (card: PluginCard) => void;
  connectSheet?: ReactNode;
}) {
  const [detail, setDetail] = useState<PluginDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState("teammates");
  const [confirm, setConfirm] = useState(false);
  const { show } = useToast();
  const load = useCallback(() => {
    void invoke<PluginDetail>("plugin_detail", { connectionId }).then(
      (d) => {
        setDetail(d);
        setError(null);
      },
      (e: IpcError) => setError(e.message),
    );
  }, [connectionId]);
  useEffect(() => {
    load();
    let off: (() => void) | undefined;
    let gone = false;
    void listen("plugins:changed", load).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
    };
  }, [load]);

  const back = (
    <button
      type="button"
      onClick={onBack}
      className="inline-flex h-7 items-center gap-1 self-start rounded-sm px-1.5 text-sm text-fg-muted hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
    >
      <ChevronLeft size={14} aria-hidden="true" />
      {t("plugins.back")}
    </button>
  );
  if (error) {
    return (
      <div className="flex flex-col gap-3">
        {back}
        <Banner tone="error" action={{ label: t("plugins.retry"), onClick: load }}>
          {error}
        </Banner>
      </div>
    );
  }
  if (!detail) {
    return (
      <div className="flex flex-col gap-3" aria-busy="true">
        {back}
        <Skeleton height={24} width={240} />
        <Skeleton height={120} />
      </div>
    );
  }
  const { card } = detail;
  const setOptions = (next: PluginOptions) => {
    setDetail({ ...detail, options: next });
    void invoke("plugin_set_option", { connectionId, ...next }).catch((e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  };
  const disconnect = () =>
    void invoke("plugin_disconnect", { connectionId }).then(onBack, (e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  const method = card.authKind === "api_key" ? "key" : card.authKind === "none" ? "none" : "oauth";

  return (
    <div className="flex flex-col gap-3" data-testid="plugin-detail">
      {back}
      <div className="flex items-center gap-2">
        <EngineGlyph letters={card.logo} engine={card.pluginId} size={20} />
        <h2 className="text-lg font-medium text-fg">{card.label}</h2>
        {card.state === "connected" ? (
          <Badge variant="done" dot>
            {t("plugins.state.connected")}
          </Badge>
        ) : (
          <Badge variant="error" dot>
            {t("plugins.state.needsAttention")}
          </Badge>
        )}
        <span className="flex-1" />
        <Menu
          trigger={<IconButton icon={<MoreHorizontal size={16} />} label={t("plugins.more")} />}
          entries={[
            { id: "reconnect", label: t("plugins.reconnect"), onSelect: () => onReconnect(card) },
            {
              id: "disconnect",
              label: t("plugins.disconnect"),
              danger: true,
              onSelect: () => setConfirm(true),
            },
          ]}
        />
      </div>
      {card.state === "needs_attention" && (
        <Banner tone="error" action={{ label: t("plugins.reconnect"), onClick: () => onReconnect(card) }}>
          {t("plugins.expired", { service: card.label })}
        </Banner>
      )}
      <KeyValue
        items={[
          {
            key: t("plugins.detail.signedInAs"),
            value: card.accountLabel ?? t("plugins.detail.unknownAccount"),
          },
          { key: t("plugins.detail.method"), value: t(`plugins.method.${method}` as MessageKey) },
          { key: t("plugins.detail.connected"), value: new Date(detail.createdAt).toLocaleDateString() },
          { key: t("plugins.detail.tools"), value: String(detail.tools.length) },
          { key: t("plugins.detail.credentials"), value: t("plugins.detail.inKeychain") },
        ]}
      />
      <Tabs
        label={t("plugins.detail.tabs")}
        value={tab}
        onValueChange={setTab}
        items={[
          { id: "teammates", label: t("plugins.detail.teammates") },
          { id: "approvals", label: t("plugins.detail.approvals") },
          { id: "tools", label: t("plugins.detail.toolsTab") },
        ]}
        renderPanel={(id) => (
          <div className="pt-3">
            {id === "teammates" &&
              (detail.teammates.length === 0 ? (
                <p className="text-sm text-fg-muted">{t("plugins.detail.noTeammates")}</p>
              ) : (
                <div className="flex flex-col gap-1">
                  <p className="text-sm text-fg-muted">
                    {t("plugins.detail.enabledFor", {
                      n: detail.teammates.filter((m) => m.granted).length,
                      total: detail.teammates.length,
                    })}
                  </p>
                  {detail.teammates.map((m) => (
                    <Switch
                      key={m.teammateId}
                      label={m.name}
                      checked={m.granted}
                      onCheckedChange={(v) => {
                        setDetail({
                          ...detail,
                          teammates: detail.teammates.map((x) =>
                            x.teammateId === m.teammateId ? { ...x, granted: v } : x,
                          ),
                        });
                        void invoke("grants_set", {
                          subject: { type: "teammate", teammateId: m.teammateId },
                          connectionId,
                          granted: v,
                        }).catch((e: IpcError) => show({ tone: "error", title: e.message }));
                      }}
                    />
                  ))}
                </div>
              ))}
            {id === "approvals" && (
              <div className="flex flex-col gap-2" data-testid="plugin-policy">
                <table className="text-sm">
                  <tbody>
                    {POLICY.map((row) => (
                      <tr key={row.risk} className="h-7">
                        <td className="pr-3 text-fg">{riskLabel(row.risk)}</td>
                        <td className="pr-3 text-fg-muted">
                          {t(`plugins.riskDesc.${row.risk}` as MessageKey)}
                        </td>
                        <td className="text-fg">
                          <span className="inline-flex items-center gap-1">
                            {policyText(row.risk)}
                            {row.locked && <Lock size={12} aria-label={t("plugins.policy.locked")} />}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <Switch
                  label={t("plugins.policy.writesInFull")}
                  checked={detail.options.allowWritesInFull}
                  onCheckedChange={(v) => setOptions({ ...detail.options, allowWritesInFull: v })}
                />
                <Switch
                  label={t("plugins.policy.publishInFull")}
                  checked={detail.options.allowPublishInFull}
                  onCheckedChange={(v) => setOptions({ ...detail.options, allowPublishInFull: v })}
                />
                <p className="text-sm text-fg-muted">{t("plugins.policy.unattended")}</p>
              </div>
            )}
            {id === "tools" &&
              (detail.toolsError ? (
                <Banner tone="warning">{detail.toolsError}</Banner>
              ) : (
                <ul className="flex flex-col gap-1">
                  {detail.tools.map((tool) => (
                    <li key={tool.name} className="flex items-center gap-2 text-sm">
                      <code className="font-mono text-fg">{tool.name}</code>
                      <Badge variant={riskVariant(tool.riskClass)}>{riskLabel(tool.riskClass)}</Badge>
                      <span className="min-w-0 flex-1 truncate text-fg-muted">{tool.description}</span>
                    </li>
                  ))}
                </ul>
              ))}
          </div>
        )}
      />
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={t("plugins.disconnectTitle", { service: card.label })}
        body={t("plugins.disconnectBody")}
        confirmLabel={t("plugins.disconnect.confirm")}
        cancelLabel={t("plugins.connect.cancel")}
        danger
        onConfirm={disconnect}
      />
      {connectSheet}
    </div>
  );
}
