// UI-51 routine editor (F-RT-01): teammate, name, an instruction that has
// to stand on its own, the schedule (CronInput), access (Full only with the
// explicit "act without asking" choice) and background runs. Esc on a
// changed form asks "Discard changes?".
import { useState } from "react";
import type { Access, Routine, RoutineInput } from "@tandem/protocol";
import { Button, Callout, Checkbox, Input, Select, Sheet, Textarea, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useNewTeammate } from "../agents/NewTeammateSheet";
import { isIpcError, useRoutines } from "../../stores/routines";
import { useSession } from "../../stores/session";
import { useTeammates } from "../../stores/teammates";
import { CronInput, draftOf, specOf, type CronDraft } from "./CronInput";
import { localZone } from "./schedule";

/** What the editor opens with: a routine to edit, or a new one's start. */
export type EditorSeed = { routine?: Routine; name?: string; instruction?: string };

/** The guidance minimum: enough for the run to stand on its own. */
export const MIN_INSTRUCTION = 20;

type Form = {
  teammateId: string;
  name: string;
  instruction: string;
  cron: CronDraft;
  access: Access;
  fullOk: boolean;
  background: boolean;
};

function initial(seed: EditorSeed, firstTeammate: string): Form {
  const r = seed.routine;
  return {
    teammateId: r?.teammateId ?? firstTeammate,
    name: r?.name ?? seed.name ?? "",
    instruction: r?.instruction ?? seed.instruction ?? "",
    cron: draftOf(
      r?.schedule ?? {
        schedule: { kind: "weekly", days: [1, 2, 3, 4, 5], times: ["09:00"] },
        timezone: localZone(),
      },
    ),
    access: r?.access ?? "edits",
    fullOk: r?.access === "full",
    background: r?.runMode === "background",
  };
}

export function RoutineEditor({ seed, onClose }: { seed: EditorSeed; onClose: () => void }) {
  const teammates = useTeammates((s) => s.list).filter((s) => s.chatCapable);
  const limits = useSession((s) => s.entitlement?.limits);
  const { show: toast } = useToast();
  const [start] = useState(() => initial(seed, teammates[0]?.teammate.id ?? ""));
  const [form, setForm] = useState<Form>(start);
  const [busy, setBusy] = useState(false);
  const [touched, setTouched] = useState(false);
  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));
  const dirty = JSON.stringify(form) !== JSON.stringify(start);
  const teammateId = form.teammateId || teammates[0]?.teammate.id || "";
  const short = form.instruction.trim().length < MIN_INSTRUCTION;
  const canSave =
    !busy && !!teammateId && form.name.trim() !== "" && !short && (form.access !== "full" || form.fullOk);
  const save = async (runNow: boolean) => {
    setTouched(true);
    if (!canSave) return;
    setBusy(true);
    const input: RoutineInput = {
      id: seed.routine?.id,
      teammateId,
      name: form.name.trim(),
      instruction: form.instruction,
      schedule: specOf(form.cron),
      access: form.access,
      fullConfirmed: form.access === "full" && form.fullOk,
      runMode: form.background ? "background" : "in_app",
    };
    const saved = await useRoutines.getState().save(input);
    setBusy(false);
    if (isIpcError(saved)) {
      toast({ tone: "error", title: saved.message });
      return;
    }
    if (runNow) {
      const e = await useRoutines.getState().runNow(saved.id);
      if (e) toast({ tone: "error", title: e.message });
    }
    onClose();
  };
  return (
    <Sheet
      open
      onOpenChange={(o) => !o && onClose()}
      title={seed.routine ? t("routines.editor.editTitle") : t("routines.editor.newTitle")}
      dirty={dirty}
      closeLabel={t("routines.cancel")}
      discardPrompt={{
        title: t("routines.editor.discard"),
        confirm: t("routines.editor.discardConfirm"),
        cancel: t("routines.editor.keepEditing"),
      }}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("routines.cancel")}
          </Button>
          <Button
            variant="secondary"
            disabled={!canSave}
            onClick={() => void save(true)}
            data-testid="routine-save-run"
          >
            {t("routines.editor.saveRun")}
          </Button>
          <Button
            variant="primary"
            disabled={!canSave}
            onClick={() => void save(false)}
            data-testid="routine-save"
          >
            {t("routines.editor.save")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-4"
        data-testid="routine-editor"
        onSubmit={(e) => {
          e.preventDefault();
          void save(false);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void save(false);
          }
        }}
      >
        {teammates.length === 0 ? (
          <Callout
            tone="info"
            actions={
              <Button size="sm" variant="secondary" onClick={() => useNewTeammate.getState().show(true)}>
                {t("routines.editor.newTeammate")}
              </Button>
            }
          >
            {t("routines.editor.noTeammate")}
          </Callout>
        ) : (
          <Select
            label={t("routines.editor.teammate")}
            value={teammateId}
            options={teammates.map((s) => ({ value: s.teammate.id, label: s.teammate.name }))}
            onValueChange={(v) => set({ teammateId: v })}
          />
        )}
        <Input
          label={t("routines.editor.name")}
          value={form.name}
          maxLength={80}
          onChange={(e) => set({ name: e.target.value })}
          error={touched && form.name.trim() === "" ? t("routines.editor.nameRequired") : undefined}
          data-testid="routine-name-input"
        />
        <Textarea
          label={t("routines.editor.instruction")}
          minRows={5}
          value={form.instruction}
          onChange={(e) => set({ instruction: e.target.value })}
          hint={t("routines.editor.tip")}
          error={touched && short ? t("routines.editor.tooShort") : undefined}
          data-testid="routine-instruction"
        />
        <CronInput value={form.cron} onChange={(cron) => set({ cron })} />
        <Select
          label={t("routines.editor.access")}
          value={form.access}
          options={[
            { value: "read", label: t("routines.access.read") },
            { value: "edits", label: t("routines.access.edits") },
            { value: "full", label: t("routines.access.full") },
          ]}
          onValueChange={(v) => set({ access: v as Access, fullOk: false })}
        />
        {form.access === "full" && (
          <Callout tone="warning" title={t("routines.editor.fullWarning")}>
            <Checkbox
              label={t("routines.editor.fullConfirm")}
              checked={form.fullOk}
              onCheckedChange={(v) => set({ fullOk: v })}
            />
          </Callout>
        )}
        <Checkbox
          label={t("routines.editor.background")}
          description={
            limits?.backgroundRoutines
              ? t("routines.editor.backgroundDesc")
              : t("routines.editor.backgroundPro")
          }
          checked={form.background}
          disabled={!limits?.backgroundRoutines}
          onCheckedChange={(v) => set({ background: v })}
        />
      </form>
    </Sheet>
  );
}
