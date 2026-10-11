// The prompt library's picker (F-EX-05, UI-145): saved prompts from the
// workspace's .tandem/prompts and the profile, filled in and inserted into
// the composer or pane on screen. Inserting never sends.
import { useEffect, useMemo, useState } from "react";
import { create } from "zustand";
import type { FileContext, IpcError, PromptInfo, PromptList, Rendered } from "@tandem/protocol";
import { Badge, Button, Callout, Dialog, EmptyState, Input, QuickPicker, SegmentedControl, Spinner, Textarea, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { readText } from "../../ipc/clipboard";
import { invoke } from "../../ipc/invoke";
import { useFiles, contextKey } from "../../stores/files";
import { useSidebar } from "../../stores/sidebar";
import { terminalHandles } from "../code/TerminalView";
import { activeFiles } from "../files/context";
import { currentTarget, insertText, type InsertTarget } from "../insert/target";

type PromptsStore = {
  open: boolean;
  target: InsertTarget;
  context: FileContext | null;
  show: () => void;
  close: () => void;
};

const NEW = "__new";

export const usePrompts = create<PromptsStore>((set) => ({
  open: false,
  target: { kind: "clipboard" },
  context: null,
  show: () => {
    const active = activeFiles();
    set({ open: true, target: currentTarget(), context: active.kind === "context" ? active.context : null });
  },
  close: () => set({ open: false }),
}));

/** What the UI knows for the built-in variables: the pane's selection and
 * the active editor file. The core adds the branch; the clipboard is read
 * only when the prompt uses it. */
async function knownValues(prompt: PromptInfo, target: InsertTarget, context: FileContext | null): Promise<Record<string, string>> {
  const values: Record<string, string> = {};
  if (prompt.variables.includes("selection") && target.kind === "pane") {
    values.selection = terminalHandles.get(target.paneId)?.selection() ?? "";
  }
  if (prompt.variables.includes("file") && context) {
    const state = useFiles.getState().byKey[contextKey(context)];
    if (state?.active) values.file = state.active;
  }
  if (prompt.variables.includes("clipboard")) values.clipboard = await readText();
  return values;
}

function matches(p: PromptInfo, q: string): boolean {
  const hay = `${p.title} ${p.description ?? ""}`.toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

function FillDialog({
  prompt,
  values,
  missing,
  context,
  onInsert,
  onClose,
}: {
  prompt: PromptInfo;
  values: Record<string, string>;
  missing: string[];
  context: FileContext | null;
  onInsert: (text: string) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<Record<string, string>>(values);
  const [preview, setPreview] = useState("");
  const names = prompt.variables;
  // Renders with the values as they are now: the preview may lag behind.
  const submit = async () => {
    try {
      const r = await invoke<Rendered>("prompts_render", { path: prompt.path, context, values: draft });
      onInsert(r.text);
    } catch {
      onInsert(preview);
    }
  };
  useEffect(() => {
    const timer = setTimeout(() => {
      void invoke<Rendered>("prompts_render", { path: prompt.path, context, values: draft }).then(
        (r) => setPreview(r.text),
        () => undefined,
      );
    }, 120);
    return () => clearTimeout(timer);
  }, [draft, prompt.path, context]);
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("prompts.fill.title")}
      description={prompt.title}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("prompts.cancel")}
          </Button>
          <Button variant="primary" data-testid="prompt-insert" onClick={() => void submit()}>
            {t("prompts.insert")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {names.map((name, i) => (
          <Input
            key={name}
            label={`{{${name}}}`}
            value={draft[name] ?? ""}
            autoFocus={i === names.findIndex((n) => missing.includes(n))}
            data-testid="prompt-var"
            data-name={name}
            onChange={(e) => setDraft({ ...draft, [name]: e.target.value })}
          />
        ))}
        <Textarea label={t("prompts.preview")} value={preview} readOnly rows={6} />
      </form>
    </Dialog>
  );
}

function NewDialog({ context, onClose }: { context: FileContext | null; onClose: () => void }) {
  const [name, setName] = useState("");
  const [scope, setScope] = useState<"repo" | "profile">(context ? "repo" : "profile");
  const [error, setError] = useState<string | null>(null);
  const create_ = async () => {
    try {
      const { path } = await invoke<{ path: string }>("prompts_create", { context, scope, name });
      onClose();
      // Opens it in the Files editor.
      if (context) {
        useSidebar.getState().openTab("files");
        useFiles.getState().openLink(contextKey(context), path);
      }
    } catch (e) {
      setError((e as IpcError).message ?? String(e));
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("prompts.new.title")}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("prompts.cancel")}
          </Button>
          <Button variant="primary" data-testid="prompt-create" onClick={() => void create_()}>
            {t("prompts.new.create")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          void create_();
        }}
      >
        <Input label={t("prompts.new.name")} value={name} autoFocus error={error ?? undefined} onChange={(e) => setName(e.target.value)} data-testid="prompt-name" />
        <SegmentedControl
          label={t("prompts.new.where")}
          value={scope}
          onValueChange={(v) => setScope(v as "repo" | "profile")}
          items={[...(context ? [{ id: "repo", label: t("prompts.source.repo") }] : []), { id: "profile", label: t("prompts.source.profile") }]}
        />
      </form>
    </Dialog>
  );
}

export function PromptPicker() {
  const { open, target, context, close } = usePrompts();
  const [list, setList] = useState<PromptList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [value, setValue] = useState("");
  const [fill, setFill] = useState<{ prompt: PromptInfo; values: Record<string, string>; missing: string[] } | null>(null);
  const [creating, setCreating] = useState(false);
  const { show } = useToast();

  const load = () =>
    void invoke<PromptList>("prompts_list", { context }).then(
      (l) => {
        setList(l);
        setError(null);
      },
      (e: IpcError) => setError(e.message),
    );

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setList(null);
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const shown = useMemo(() => (list?.items ?? []).filter((p) => matches(p, query)), [list, query]);

  const deliver = (text: string) => {
    void insertText(target, text).then((how) => {
      if (how === "copied") show({ tone: "success", title: t("prompts.copied") });
      if (how === "failed") show({ tone: "error", title: t("prompts.copyFailed") });
    });
  };

  const choose = async (id: string, mod: boolean) => {
    if (id === NEW) {
      close();
      setCreating(true);
      return;
    }
    const prompt = list?.items.find((p) => p.path === id);
    if (!prompt) return;
    close();
    if (mod) {
      // Mod+Enter: edit it in the Files editor.
      if (context) {
        useSidebar.getState().openTab("files");
        useFiles.getState().openLink(contextKey(context), prompt.path);
      }
      return;
    }
    try {
      const values = await knownValues(prompt, target, context);
      const r = await invoke<Rendered>("prompts_render", { path: prompt.path, context, values });
      if (r.missing.length) setFill({ prompt, values: r.values, missing: r.missing });
      else deliver(r.text);
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message ?? String(e) });
    }
  };

  let state;
  if (error) state = <EmptyState title={t("prompts.error")} body={error} primary={<Button size="sm" onClick={load}>{t("prompts.retry")}</Button>} />;
  else if (!list)
    state = (
      <div className="flex items-center gap-2 p-3 text-sm text-fg-muted">
        <Spinner size={12} /> {t("prompts.loading")}
      </div>
    );
  else if (list.items.length === 0 && !query)
    state = undefined; // the "New prompt…" row shows with the empty note below
  else if (shown.length === 0) state = <p className="px-3 py-6 text-center text-base text-fg-muted">{t("prompts.noMatch", { q: query })}</p>;

  const items = [
    ...shown.map((p) => ({
      id: p.path,
      label: p.title,
      content: (
        <div className="flex min-w-0 flex-1 items-center gap-2" data-testid="prompt-row" data-title={p.title} data-source={p.source}>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-fg">{p.title}</span>
            {p.description && <span className="truncate text-xs text-fg-muted">{p.description}</span>}
          </span>
          <Badge variant="outline">{p.source === "repo" ? t("prompts.source.repo") : t("prompts.source.profile")}</Badge>
        </div>
      ),
    })),
    { id: NEW, label: t("prompts.new"), content: <span className="text-fg-muted" data-testid="prompt-new">{t("prompts.new")}</span> },
  ];

  const header =
    list && (list.excludeHidesRepo || (list.items.length === 0 && !query) || list.skipped.length > 0) ? (
      <div className="flex flex-col gap-2">
        {list.items.length === 0 && !query && (
          <p className="text-sm text-fg-muted" data-testid="prompts-empty">
            {t("prompts.empty")} · {t("prompts.empty.body")}
          </p>
        )}
        {list.excludeHidesRepo && context && (
          <Callout
            tone="info"
            actions={
              <Button size="sm" variant="secondary" onClick={() => void invoke("prompts_share_repo", { context }).then(load, (e: IpcError) => show({ tone: "error", title: e.message }))}>
                {t("prompts.share")}
              </Button>
            }
          >
            {t("prompts.notShared")}
          </Callout>
        )}
        {list.skipped.length > 0 && <p className="text-xs text-fg-muted">{t("prompts.skipped", { n: list.skipped.length })}</p>}
      </div>
    ) : undefined;

  return (
    <>
      <QuickPicker
        open={open}
        onOpenChange={(o) => !o && close()}
        title={t("prompts.title")}
        placeholder={t("prompts.placeholder")}
        query={query}
        onQueryChange={setQuery}
        value={value}
        onValueChange={setValue}
        items={state ? [] : items}
        state={state}
        header={header}
        onSelect={(id, mod) => void choose(id, mod)}
        hints={[
          { keys: ["Enter"], label: target.kind === "clipboard" ? t("prompts.hint.copy") : t("prompts.hint.insert") },
          ...(context ? [{ keys: ["Mod", "Enter"], label: t("prompts.hint.edit") }] : []),
        ]}
      />
      {fill && (
        <FillDialog
          prompt={fill.prompt}
          values={fill.values}
          missing={fill.missing}
          context={context}
          onClose={() => setFill(null)}
          onInsert={(text) => {
            setFill(null);
            deliver(text);
          }}
        />
      )}
      {creating && <NewDialog context={context} onClose={() => setCreating(false)} />}
    </>
  );
}
