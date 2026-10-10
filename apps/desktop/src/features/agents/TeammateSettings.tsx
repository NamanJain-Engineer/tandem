import { useEffect, useMemo, useState } from "react";
import type {
  Access,
  IpcError,
  LearnMode,
  ModelEntry,
  PluginCard,
  Teammate,
  TeammatePatch,
  TeammateSummary,
} from "@tandem/protocol";
import {
  Avatar,
  Button,
  Callout,
  ConfirmDialog,
  Input,
  SegmentedControl,
  Select,
  SettingsGroup,
  SettingsRow,
  Switch,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { accountsFor, useAccounts } from "../../stores/accounts";
import { useSession } from "../../stores/session";
import { useEngines } from "../../stores/engines";
import { useTeammates } from "../../stores/teammates";
import { useUi } from "../../stores/ui";
import { HuePicker, Places } from "./NewTeammateSheet";

type Form = {
  name: string;
  avatarHue: number | null;
  brief: string;
  engineId: string;
  model: string | null;
  preferredAccountId: string | null;
  places: string[];
  defaultAccess: Access;
  memoryCap: number;
  learnMode: LearnMode;
  mayMessage: boolean;
  maySchedule: boolean;
  plugins: string[];
};

const CAPS = [2048, 4096, 8192, 16384];

function formOf(tm: Teammate, plugins: string[]): Form {
  return {
    name: tm.name,
    avatarHue: tm.avatarHue,
    brief: tm.brief,
    engineId: tm.engineId,
    model: tm.model,
    preferredAccountId: tm.preferredAccountId,
    places: tm.places,
    defaultAccess: tm.settings.defaultAccess,
    memoryCap: tm.settings.memoryCap,
    learnMode: tm.settings.learnMode,
    mayMessage: tm.settings.mayMessage,
    maySchedule: tm.settings.maySchedule,
    plugins,
  };
}

function same(a: Form, b: Form): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The patch that turns `base` into `next`. */
export function patchOf(tm: Teammate, base: Form, next: Form): TeammatePatch {
  const p: TeammatePatch = {};
  if (next.name !== base.name) p.name = next.name.trim();
  if (next.avatarHue !== base.avatarHue) p.avatarHue = next.avatarHue;
  if (next.brief !== base.brief) p.brief = next.brief;
  if (next.engineId !== base.engineId) p.engineId = next.engineId;
  if (next.model !== base.model) p.model = next.model;
  if (next.preferredAccountId !== base.preferredAccountId) p.preferredAccountId = next.preferredAccountId;
  if (JSON.stringify(next.places) !== JSON.stringify(base.places)) p.places = next.places;
  if (
    next.defaultAccess !== base.defaultAccess ||
    next.memoryCap !== base.memoryCap ||
    next.learnMode !== base.learnMode ||
    next.mayMessage !== base.mayMessage ||
    next.maySchedule !== base.maySchedule
  ) {
    p.settings = {
      ...tm.settings,
      defaultAccess: next.defaultAccess,
      memoryCap: next.memoryCap,
      learnMode: next.learnMode,
      mayMessage: next.mayMessage,
      maySchedule: next.maySchedule,
    };
  }
  return p;
}

/** UI-75/76: a teammate's settings with a Save / Revert bar. */
export function TeammateSettings({ s }: { s: TeammateSummary }) {
  const tm = s.teammate;
  const engines = useEngines((st) => st.list);
  const enginesLoaded = useEngines((st) => st.loaded);
  // Nothing else may have listed the engines yet (a launch straight into Agent mode).
  useEffect(() => {
    if (!enginesLoaded) void useEngines.getState().refresh();
  }, [enginesLoaded]);
  const accounts = useAccounts((st) => st.list);
  // Only where routines can run: the plan has them (D-909).
  const routines = useSession((st) => !!st.entitlement?.limits.routines);
  const update = useTeammates((st) => st.update);
  const remove = useTeammates((st) => st.remove);
  const setDirty = useTeammates((st) => st.setDirty);
  const { show } = useToast();
  const [connected, setConnected] = useState<PluginCard[]>([]);
  const [base, setBase] = useState<Form | null>(null);
  const [form, setForm] = useState<Form | null>(null);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    let live = true;
    void Promise.all([
      invoke<PluginCard[]>("plugins_catalog").catch(() => [] as PluginCard[]),
      invoke<string[]>("grants_list", { subject: { type: "teammate", teammateId: tm.id } }).catch(
        () => [] as string[],
      ),
    ]).then(([cards, granted]) => {
      if (!live) return;
      setConnected(cards.filter((c) => c.state === "connected" && c.connectionId));
      const f = formOf(tm, granted);
      setBase(f);
      setForm((cur) => (cur && base && !same(cur, base) ? cur : f));
    });
    return () => {
      live = false;
    };
    // Reload when the teammate changes in the core.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tm]);

  useEffect(() => {
    if (!form) return;
    let live = true;
    void invoke<ModelEntry[] | null>("models_list", { engineId: form.engineId })
      .then((m) => live && setModels(Array.isArray(m) ? m : []))
      .catch(() => live && setModels([]));
    return () => {
      live = false;
    };
  }, [form?.engineId, form]);

  const dirty = !!form && !!base && !same(form, base);
  useEffect(() => {
    setDirty(dirty);
    return () => setDirty(false);
  }, [dirty, setDirty]);

  const engineOptions = useMemo(
    () =>
      engines.map((e) => ({
        value: e.spec.id,
        label: e.spec.label,
        disabled: !e.install,
        disabledReason: e.install ? undefined : t("agent.new.notInstalled"),
        group: e.chatCapable ? undefined : t("agent.new.terminalGroup"),
      })),
    [engines],
  );
  if (!form || !base) return null;
  const set = (p: Partial<Form>) => setForm({ ...form, ...p });
  const engineLabel = (id: string) => engines.find((e) => e.spec.id === id)?.spec.label ?? id;
  const engineChanged = form.engineId !== base.engineId;
  const mine = accountsFor(accounts, form.engineId);
  const nameError = !form.name.trim() ? t("agent.settings.nameRequired") : undefined;

  const save = async () => {
    setSaving(true);
    try {
      const patch = patchOf(tm, base, form);
      if (Object.keys(patch).length) {
        const r = await update(tm.id, patch);
        if ("code" in r) {
          show({ tone: "error", title: r.message });
          return;
        }
      }
      for (const c of connected) {
        const id = c.connectionId ?? "";
        const want = form.plugins.includes(id);
        if (want !== base.plugins.includes(id))
          await invoke("grants_set", {
            subject: { type: "teammate", teammateId: tm.id },
            connectionId: id,
            granted: want,
          });
      }
      setBase(form);
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="relative flex min-h-0 flex-col" data-testid="teammate-settings">
      <div className="flex flex-col gap-6 overflow-y-auto p-4 pb-20">
        <SettingsGroup label={t("agent.settings.identity")}>
          <SettingsRow
            label={t("agent.settings.name")}
            control={() => (
              <Input
                label={t("agent.settings.name")}
                hideLabel
                maxLength={40}
                value={form.name}
                onChange={(e) => set({ name: e.target.value })}
                error={nameError}
                data-testid="settings-name"
              />
            )}
          />
          <SettingsRow
            label={t("agent.settings.avatar")}
            control={() => (
              <span className="flex items-center gap-3">
                <Avatar name={form.name || tm.name} id={tm.id} hue={form.avatarHue} size={32} />
                <HuePicker value={form.avatarHue} onChange={(h) => set({ avatarHue: h })} />
              </span>
            )}
          />
          <SettingsRow
            label={t("agent.settings.brief")}
            description={t("agent.settings.briefHint", { name: tm.name })}
            control={() => (
              <Textarea
                label={t("agent.settings.brief")}
                hideLabel
                minRows={8}
                maxRows={16}
                maxLength={8000}
                value={form.brief}
                onChange={(e) => set({ brief: e.target.value })}
                className="w-[28rem]"
              />
            )}
          />
        </SettingsGroup>
        <SettingsGroup label={t("agent.settings.engineGroup")}>
          <SettingsRow
            label={t("agent.settings.engine")}
            control={() => (
              <Select
                ariaLabel={t("agent.settings.engine")}
                options={engineOptions}
                value={form.engineId}
                onValueChange={(v) => set({ engineId: v, model: null, preferredAccountId: null })}
              />
            )}
          />
          {engineChanged && (
            <div className="px-3 pb-3">
              <Callout tone="warning">{t("agent.settings.engineChange")}</Callout>
            </div>
          )}
          <SettingsRow
            label={t("agent.settings.model")}
            control={() => (
              <Select
                ariaLabel={t("agent.settings.model")}
                value={form.model ?? "default"}
                options={[
                  { value: "default", label: t("agent.settings.modelDefault") },
                  ...models.filter((m) => m.id !== "default").map((m) => ({ value: m.id, label: m.label })),
                ]}
                onValueChange={(v) => set({ model: v === "default" ? null : v })}
              />
            )}
          />
          <SettingsRow
            label={t("agent.settings.account")}
            description={t("agent.settings.accountHint")}
            control={() => (
              <Select
                ariaLabel={t("agent.settings.account")}
                value={form.preferredAccountId ?? "default"}
                options={[
                  { value: "default", label: t("agent.settings.accountDefault") },
                  ...mine.map((a) => ({ value: a.id, label: a.label })),
                ]}
                onValueChange={(v) => set({ preferredAccountId: v === "default" ? null : v })}
              />
            )}
          />
        </SettingsGroup>
        <SettingsGroup label={t("agent.settings.placesGroup")}>
          <div className="flex flex-col gap-2 p-3">
            <span className="text-sm text-fg-muted">{t("agent.settings.home", { path: tm.home })}</span>
            <Places places={form.places} onChange={(places) => set({ places })} />
          </div>
        </SettingsGroup>
        <SettingsGroup label={t("agent.settings.permissions")}>
          <SettingsRow
            label={t("agent.settings.access")}
            description={t("agent.settings.accessHint")}
            control={() => (
              <Select
                ariaLabel={t("agent.settings.access")}
                value={form.defaultAccess}
                options={[
                  { value: "read", label: t("thread.access.read") },
                  { value: "edits", label: t("thread.access.edits") },
                  { value: "full", label: t("thread.access.full") },
                ]}
                onValueChange={(v) => set({ defaultAccess: v as Access })}
              />
            )}
          />
          <SettingsRow
            label={t("agent.settings.mayMessage")}
            description={t("agent.settings.mayMessageHint")}
            control={() => (
              <Switch
                checked={form.mayMessage}
                onCheckedChange={(v) => set({ mayMessage: v })}
                aria-label={t("agent.settings.mayMessage")}
              />
            )}
          />
          {routines && (
            <SettingsRow
              label={t("agent.settings.maySchedule")}
              description={t("agent.settings.mayScheduleHint")}
              control={() => (
                <Switch
                  checked={form.maySchedule}
                  onCheckedChange={(v) => set({ maySchedule: v })}
                  aria-label={t("agent.settings.maySchedule")}
                />
              )}
            />
          )}
          <div className="flex flex-col gap-1 px-3 py-2.5" data-testid="settings-plugins">
            <span className="text-base text-fg">{t("agent.settings.plugins")}</span>
            <span className="text-sm text-fg-muted">{t("agent.settings.pluginsHint")}</span>
            {connected.length === 0 ? (
              <span className="text-sm text-fg-muted">{t("agent.settings.noPlugins")}</span>
            ) : (
              connected.map((c) => (
                <label
                  key={c.connectionId}
                  className="flex items-center justify-between gap-3 py-1 text-base text-fg"
                >
                  <span>
                    {c.label}
                    {c.accountLabel ? ` · ${c.accountLabel}` : ""}
                  </span>
                  <Switch
                    checked={form.plugins.includes(c.connectionId ?? "")}
                    aria-label={t("agent.settings.pluginFor", { plugin: c.label, name: tm.name })}
                    onCheckedChange={(v) =>
                      set({
                        plugins: v
                          ? [...form.plugins, c.connectionId ?? ""]
                          : form.plugins.filter((x) => x !== c.connectionId),
                      })
                    }
                  />
                </label>
              ))
            )}
            <span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => useUi.getState().openSettings("plugins")}
                data-testid="connect-more-plugins"
              >
                {t("agent.settings.connectMore")}
              </Button>
            </span>
          </div>
        </SettingsGroup>
        <SettingsGroup label={t("agent.settings.learning")}>
          <SettingsRow
            label={t("agent.settings.learn")}
            description={t(`agent.settings.learn.${form.learnMode}`, { name: tm.name })}
            control={() => (
              <SegmentedControl
                size="sm"
                label={t("agent.settings.learn")}
                value={form.learnMode}
                onValueChange={(v) => set({ learnMode: v as LearnMode })}
                items={[
                  { id: "ask", label: t("agent.settings.ask") },
                  { id: "auto", label: t("agent.settings.auto") },
                  { id: "off", label: t("agent.settings.off") },
                ]}
              />
            )}
          />
          <p className="px-3 pb-3 text-sm text-fg-muted">{t("agent.settings.learnNote")}</p>
          <SettingsRow
            label={t("agent.settings.cap")}
            description={t("agent.settings.capHint")}
            control={() => (
              <SegmentedControl
                size="sm"
                label={t("agent.settings.cap")}
                value={String(form.memoryCap)}
                onValueChange={(v) => set({ memoryCap: Number(v) })}
                items={CAPS.map((c) => ({ id: String(c), label: t("agent.settings.capN", { n: c / 1024 }) }))}
              />
            )}
          />
        </SettingsGroup>
        <SettingsGroup label={t("agent.settings.danger")}>
          <SettingsRow
            label={t("agent.settings.delete")}
            description={t("agent.settings.deleteHint")}
            control={() => (
              <Button variant="danger" onClick={() => setDeleting(true)} data-testid="settings-delete">
                {t("agent.settings.deleteButton")}
              </Button>
            )}
          />
        </SettingsGroup>
      </div>
      {dirty && (
        <div
          className="absolute inset-x-0 bottom-0 flex items-center gap-2 border-t border-border bg-surface-3 px-4 py-2"
          data-testid="save-bar"
        >
          <span className="min-w-0 flex-1 text-base text-fg">{t("agent.settings.unsaved")}</span>
          <Button variant="ghost" onClick={() => setForm(base)}>
            {t("agent.settings.revert")}
          </Button>
          <Button
            variant="primary"
            disabled={saving || !!nameError}
            onClick={() => void save()}
            data-testid="settings-save"
          >
            {engineChanged
              ? t("agent.settings.saveSwitch", { engine: engineLabel(form.engineId) })
              : t("agent.settings.save")}
          </Button>
        </div>
      )}
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        danger
        title={t("agent.rail.deleteTitle", { name: tm.name })}
        body={t("agent.rail.deleteBody")}
        confirmLabel={t("agent.rail.deleteOk")}
        requireText={tm.name}
        requireLabel={(text) => t("agent.settings.typeName", { name: text })}
        onConfirm={() => {
          setDeleting(false);
          void remove(tm.id).then((e) => e && show({ tone: "error", title: e.message }));
        }}
      />
    </div>
  );
}
