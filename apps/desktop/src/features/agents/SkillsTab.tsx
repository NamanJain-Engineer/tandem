import { useCallback, useEffect, useMemo, useState } from "react";
import { Check, Library, MoreHorizontal, Plus, Sparkles, X } from "lucide-react";
import type { IpcError, Skill, SkillEntry, Teammate } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  ConfirmDialog,
  Dialog,
  EmptyState,
  IconButton,
  Input,
  Menu,
  Sheet,
  Skeleton,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useTeammates } from "../../stores/teammates";
import {
  DESCRIPTION_MAX,
  NAME_MAX,
  SLUG,
  TEMPLATE_BODY,
  checklist,
  joinSkill,
  splitSkill,
  type SkillFields,
} from "./skillText";

type Editing = { slug: string | null; fields: SkillFields | null; raw: string };

/** UI-74: a teammate's skills, the editor and the starter library. */
export function SkillsTab({ teammate }: { teammate: Teammate }) {
  const version = useTeammates((s) => s.version);
  const { show } = useToast();
  const [skills, setSkills] = useState<SkillEntry[] | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [library, setLibrary] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setSkills(await invoke<SkillEntry[]>("skills_list", { teammateId: teammate.id }));
      setError(null);
    } catch (e) {
      setError(e as IpcError);
    }
  }, [teammate.id]);
  useEffect(() => {
    void load();
  }, [load, version]);

  const fail = (e: unknown) => show({ tone: "error", title: (e as IpcError).message });

  const open = async (slug: string) => {
    try {
      const skill = await invoke<Skill>("skill_get", { teammateId: teammate.id, slug });
      setEditing({ slug, fields: splitSkill(skill.markdown), raw: skill.markdown });
    } catch (e) {
      fail(e);
    }
  };

  if (error && !skills) {
    return (
      <div className="p-4">
        <Banner tone="error" action={{ label: t("agent.skills.retry"), onClick: () => void load() }}>
          {error.message}
        </Banner>
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-col gap-3 p-4" data-testid="skills-tab">
      <div className="flex items-center gap-2">
        <p className="min-w-0 flex-1 text-sm text-fg-muted">
          {t("agent.skills.intro", { name: teammate.name })}
        </p>
        <Button
          size="sm"
          variant="ghost"
          iconLeft={<Library size={14} />}
          onClick={() => setLibrary(true)}
          data-testid="skills-library"
        >
          {t("agent.skills.fromLibrary")}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          iconLeft={<Plus size={14} />}
          onClick={() =>
            setEditing({ slug: null, fields: { name: "", description: "", body: TEMPLATE_BODY }, raw: "" })
          }
          data-testid="skills-new"
        >
          {t("agent.skills.new")}
        </Button>
      </div>
      {!skills ? (
        <div className="flex flex-col gap-2" aria-busy="true" aria-label={t("agent.skills.loading")}>
          {[0, 1].map((n) => (
            <Skeleton key={n} height={44} />
          ))}
        </div>
      ) : skills.length === 0 ? (
        <EmptyState
          icon={<Sparkles size={20} />}
          title={t("agent.skills.emptyTitle")}
          body={t("agent.skills.empty", { name: teammate.name })}
        />
      ) : (
        <ul className="flex flex-col rounded-md border border-border bg-surface-2" data-testid="skills-list">
          {skills.map((s) => (
            <li
              key={s.slug}
              data-testid="skill-row"
              data-slug={s.slug}
              data-valid={s.valid ? "true" : "false"}
              className="flex items-start gap-3 border-b border-divider px-3 py-2 last:border-b-0"
            >
              <button
                type="button"
                className="flex min-w-0 flex-1 flex-col items-start gap-0.5 rounded-sm text-left focus-visible:shadow-focus focus-visible:outline-none"
                onClick={() => void open(s.slug)}
              >
                <span className="flex items-center gap-2">
                  <span className="font-mono text-base text-fg">{s.slug}</span>
                  {s.isNew && <Badge variant="accent">{t("agent.skills.newBadge")}</Badge>}
                  {!s.valid && <Badge variant="error">{t("agent.skills.invalid")}</Badge>}
                </span>
                <span className="text-sm text-fg-muted">{s.valid ? s.description : s.error}</span>
              </button>
              <Menu
                align="end"
                trigger={
                  <IconButton
                    size="sm"
                    icon={<MoreHorizontal size={14} />}
                    label={t("agent.skills.actions", { name: s.slug })}
                  />
                }
                entries={[
                  { id: "edit", label: t("agent.skills.edit"), onSelect: () => void open(s.slug) },
                  {
                    id: "remove",
                    label: t("agent.skills.remove"),
                    danger: true,
                    onSelect: () => setRemoving(s.slug),
                  },
                ]}
              />
            </li>
          ))}
        </ul>
      )}
      {editing && (
        <SkillEditor
          teammate={teammate}
          editing={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void load();
          }}
        />
      )}
      <LibraryDialog
        open={library}
        onOpenChange={setLibrary}
        have={skills?.map((s) => s.slug) ?? []}
        onAdd={async (slug) => {
          try {
            await invoke("skill_add_from_library", { teammateId: teammate.id, slug });
            await load();
          } catch (e) {
            fail(e);
          }
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(o) => !o && setRemoving(null)}
        danger
        title={t("agent.skills.removeTitle", { name: removing ?? "" })}
        body={t("agent.skills.removeBody")}
        confirmLabel={t("agent.skills.remove")}
        onConfirm={() => {
          const slug = removing;
          setRemoving(null);
          if (slug)
            void invoke("skill_remove", { teammateId: teammate.id, slug }).then(() => void load(), fail);
        }}
      />
    </div>
  );
}

function LibraryDialog({
  open,
  onOpenChange,
  have,
  onAdd,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  have: string[];
  onAdd: (slug: string) => Promise<void>;
}) {
  const [list, setList] = useState<SkillEntry[] | null>(null);
  useEffect(() => {
    if (open && !list) void invoke<SkillEntry[]>("skills_library").then(setList, () => setList([]));
  }, [open, list]);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("agent.skills.libraryTitle")}
      description={t("agent.skills.libraryBody")}
      size="lg"
    >
      {!list ? (
        <Skeleton height={120} />
      ) : (
        <ul className="flex flex-col gap-2" data-testid="library-list">
          {list.map((s) => {
            const added = have.includes(s.slug);
            return (
              <li key={s.slug} className="flex items-center gap-3 rounded-md border border-border p-3">
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="font-mono text-base text-fg">{s.slug}</span>
                  <span className="text-sm text-fg-muted">{s.description}</span>
                </span>
                <Button
                  size="sm"
                  variant={added ? "ghost" : "secondary"}
                  disabled={added}
                  iconLeft={added ? <Check size={14} /> : <Plus size={14} />}
                  onClick={() => void onAdd(s.slug)}
                  data-testid={`library-add-${s.slug}`}
                >
                  {added ? t("agent.skills.added") : t("agent.skills.add")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </Dialog>
  );
}

function SkillEditor({
  teammate,
  editing,
  onClose,
  onSaved,
}: {
  teammate: Teammate;
  editing: Editing;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { show } = useToast();
  const [fields, setFields] = useState<SkillFields | null>(editing.fields);
  const [raw, setRaw] = useState(editing.raw);
  const [secret, setSecret] = useState(false);
  const [saving, setSaving] = useState(false);
  const markdown = fields ? joinSkill(fields) : raw;
  const slug = editing.slug ?? fields?.name ?? "";
  useEffect(() => {
    let live = true;
    const id = setTimeout(() => {
      void invoke<string>("redact_text", { text: markdown })
        .then((r) => live && setSecret(r !== markdown))
        .catch(() => undefined);
    }, 200);
    return () => {
      live = false;
      clearTimeout(id);
    };
  }, [markdown]);
  const nameError =
    fields && fields.name && (!SLUG.test(fields.name) || fields.name.length > NAME_MAX)
      ? t("agent.skills.nameRule")
      : undefined;
  const descError =
    fields && fields.description.length > DESCRIPTION_MAX ? t("agent.skills.descRule") : undefined;
  const checks = useMemo(() => (fields ? checklist(fields) : []), [fields]);
  const ready =
    !!slug && !nameError && !descError && !secret && (!fields || fields.description.trim().length > 0);
  const dirty = markdown !== editing.raw;

  const save = async () => {
    setSaving(true);
    try {
      await invoke("skill_save", { teammateId: teammate.id, slug, markdown });
      onSaved();
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Sheet
      open
      onOpenChange={(o) => !o && onClose()}
      size="lg"
      dirty={dirty}
      title={editing.slug ? t("agent.skills.editTitle", { name: editing.slug }) : t("agent.skills.newTitle")}
      discardPrompt={{
        title: t("agent.skills.discard"),
        confirm: t("agent.skills.discardOk"),
        cancel: t("agent.skills.keepEditing"),
      }}
      footer={
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            {t("agent.skills.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!ready || saving}
            onClick={() => void save()}
            data-testid="skill-save"
          >
            {t("agent.skills.save")}
          </Button>
        </div>
      }
    >
      <div className="flex flex-col gap-3">
        {fields ? (
          <>
            <Input
              label={t("agent.skills.name")}
              monospace
              value={fields.name}
              disabled={!!editing.slug}
              onChange={(e) => setFields({ ...fields, name: e.target.value })}
              error={nameError}
              data-testid="skill-name"
            />
            <Input
              label={t("agent.skills.when")}
              hint={t("agent.skills.whenHint")}
              value={fields.description}
              onChange={(e) => setFields({ ...fields, description: e.target.value })}
              error={descError}
              data-testid="skill-description"
            />
            <Textarea
              label={t("agent.skills.body")}
              monospace
              minRows={10}
              maxRows={24}
              value={fields.body}
              onChange={(e) => setFields({ ...fields, body: e.target.value })}
            />
            <ul className="flex flex-col gap-1 text-sm" aria-label={t("agent.skills.checklist")}>
              {checks.map((c) => (
                <li
                  key={c.id}
                  className={
                    c.ok ? "flex items-center gap-1.5 text-fg" : "flex items-center gap-1.5 text-fg-muted"
                  }
                >
                  {c.ok ? <Check size={14} aria-hidden="true" /> : <X size={14} aria-hidden="true" />}
                  {t(`agent.skills.check.${c.id}`)}
                </li>
              ))}
              <li
                className={
                  secret ? "flex items-center gap-1.5 text-danger" : "flex items-center gap-1.5 text-fg"
                }
              >
                {secret ? <X size={14} aria-hidden="true" /> : <Check size={14} aria-hidden="true" />}
                {t("agent.skills.check.secrets")}
              </li>
            </ul>
          </>
        ) : (
          <>
            <Banner tone="info">{t("agent.skills.rawNote")}</Banner>
            <Textarea
              label={t("agent.skills.raw")}
              monospace
              minRows={14}
              maxRows={30}
              value={raw}
              onChange={(e) => setRaw(e.target.value)}
              error={secret ? t("agent.skills.secret") : undefined}
            />
          </>
        )}
      </div>
    </Sheet>
  );
}
