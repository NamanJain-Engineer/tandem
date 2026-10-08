import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Access, Attachment, IpcError, ModelEntry, PathEntry, Thread, ThreadDetail } from "@tandem/protocol";
import {
  Composer,
  ConfirmDialog,
  EngineGlyph,
  tokenCount,
  useToast,
  type ComposerHandle,
  type MentionItem,
  type SelectOption,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useSettings } from "../../stores/settings";
import { useThreads } from "../../stores/threads";
import { takeDropped, useThreadDrop } from "./drops";

/** Draft saves are debounced (F-TH-07). */
export const DRAFT_DEBOUNCE_MS = 300;

type Draft = { text: string; attachmentIds: string[] };

function parseDraft(json: string | undefined): Draft {
  try {
    const d = JSON.parse(json ?? "{}") as Partial<Draft>;
    return { text: typeof d.text === "string" ? d.text : "", attachmentIds: Array.isArray(d.attachmentIds) ? d.attachmentIds : [] };
  } catch {
    return { text: "", attachmentIds: [] };
  }
}

/** A failed send puts its text back ahead of anything typed since (F-TH-07 AC3). */
export function restoreAfterFailure(failed: string, typedSince: string): string {
  return typedSince.trim() ? `${failed}\n${typedSince}` : failed;
}

const models = new Map<string, Promise<ModelEntry[]>>();
function modelsFor(engineId: string): Promise<ModelEntry[]> {
  let p = models.get(engineId);
  if (!p) {
    p = invoke<ModelEntry[] | null>("models_list", { engineId }).then(
      (r) => (Array.isArray(r) ? r : []),
      () => [],
    );
    models.set(engineId, p);
    // The catalog refreshes every 10 minutes.
    setTimeout(() => models.delete(engineId), 10 * 60 * 1000);
  }
  return p;
}

const FULL_OK_KEY = "tandem.thread.fullAccessOk";
function fullAccessConfirmed(scope: string): boolean {
  try {
    return (JSON.parse(localStorage.getItem(FULL_OK_KEY) ?? "[]") as string[]).includes(scope);
  } catch {
    return false;
  }
}
function confirmFullAccess(scope: string): void {
  try {
    const list = JSON.parse(localStorage.getItem(FULL_OK_KEY) ?? "[]") as string[];
    localStorage.setItem(FULL_OK_KEY, JSON.stringify([...new Set([...list, scope])]));
  } catch {
    // Asked again next time.
  }
}

/** UI-22: the thread's composer, wired to its draft, attachments and turn. */
export function ThreadComposer({
  thread,
  detail,
  running,
  tokens,
  folderName,
  lastMessage,
  composerRef,
  prefill,
  visible = true,
}: {
  thread: Thread;
  detail: ThreadDetail | null;
  running: boolean;
  tokens: { input: number; output: number };
  folderName: string | null;
  lastMessage: () => string | null;
  composerRef?: React.Ref<ComposerHandle>;
  /** Text to put in the composer (suggestion chips, Edit and resend). */
  prefill?: { text: string; n: number } | null;
  /** Thread mode is showing: only then does the composer take focus. */
  visible?: boolean;
}) {
  const { show } = useToast();
  const engines = useEngines((s) => s.list);
  const sendKey = useSettings((s) => s.values.send_key);
  const send = useThreads((s) => s.send);
  const stop = useThreads((s) => s.stop);
  const update = useThreads((s) => s.update);
  const reloadDetail = useThreads((s) => s.reloadDetail);
  const [text, setText] = useState(() => parseDraft(detail?.draft).text);
  const [attachments, setAttachments] = useState<Attachment[]>(() => detail?.attachments ?? []);
  const [catalog, setCatalog] = useState<ModelEntry[]>([]);
  const [askFull, setAskFull] = useState(false);
  const dragOver = useThreadDrop((s) => s.over);
  const dropped = useThreadDrop((s) => s.dropped[thread.id]);
  useEffect(() => {
    if (dropped?.length) {
      const added = takeDropped(thread.id);
      setAttachments((a) => [...a, ...added]);
    }
  }, [dropped, thread.id]);
  const textRef = useRef(text);
  textRef.current = text;
  const loadedFor = useRef<string | null>(null);

  // A different thread, or its detail arriving: take its draft.
  useEffect(() => {
    if (!detail || loadedFor.current === thread.id) return;
    loadedFor.current = thread.id;
    setText(parseDraft(detail.draft).text);
    setAttachments(detail.attachments);
  }, [detail, thread.id]);

  useEffect(() => {
    if (prefill) setText(prefill.text);
  }, [prefill]);

  // Debounced draft save.
  useEffect(() => {
    if (loadedFor.current !== thread.id) return;
    const id = setTimeout(() => {
      const draft: Draft = { text, attachmentIds: attachments.map((a) => a.id) };
      void invoke("thread_save_draft", { threadId: thread.id, draft }).catch(() => undefined);
    }, DRAFT_DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [text, attachments, thread.id]);

  useEffect(() => {
    let live = true;
    void modelsFor(thread.engineId).then((m) => live && setCatalog(m));
    return () => {
      live = false;
    };
  }, [thread.engineId]);

  const engineLabel = engines.find((e) => e.spec.id === thread.engineId)?.spec.label ?? thread.engineId;
  const placeholder = running
    ? t("thread.composer.running")
    : folderName
      ? t("thread.composer.placeholder", { engine: engineLabel, folder: folderName })
      : t("thread.composer.placeholderNoFolder", { engine: engineLabel });

  const engineOptions: SelectOption[] = useMemo(
    () =>
      engines
        .filter((e) => e.install)
        .map((e) => ({
          value: e.spec.id,
          label: e.spec.label,
          icon: <EngineGlyph letters={e.spec.logo} size={14} />,
          disabled: !e.chatCapable,
          disabledReason: e.chatCapable ? undefined : t("thread.composer.terminalOnly"),
          group: e.chatCapable ? undefined : t("thread.composer.terminalOnly"),
        })),
    [engines],
  );
  const savedModel = thread.model ?? "default";
  const modelOptions: SelectOption[] = useMemo(() => {
    const list = catalog.length ? catalog : [{ id: "default", label: "Default", description: "", isDefault: true, effortLevels: [], contextWindow: null, source: "bundled" as const }];
    const opts = list.map((m) => ({ value: m.id, label: m.label, description: m.description || undefined }));
    if (!list.some((m) => m.id === savedModel)) {
      opts.push({ value: savedModel, label: t("thread.composer.modelGone", { model: savedModel }), description: undefined });
    }
    return opts;
  }, [catalog, savedModel]);
  const efforts = (catalog.find((m) => m.id === savedModel) ?? catalog[0])?.effortLevels ?? [];
  const effortLabel = (e: string) => (["low", "medium", "high", "max"].includes(e) ? t(`thread.effort.${e as "low"}`) : e);

  const setAccess = (v: string) => {
    const access = v as Access;
    const scope = thread.workspaceId ?? "none";
    if (access === "full" && !fullAccessConfirmed(scope)) {
      setAskFull(true);
      return;
    }
    void update(thread.id, { access });
  };

  const onSend = async () => {
    const sent = text;
    const sentAttachments = attachments;
    // Removed from the draft at once; restored ahead of new typing on failure.
    setText("");
    setAttachments([]);
    const res = await send(thread.id, sent, sentAttachments.map((a) => a.id), running);
    if ("code" in res) {
      setText((typed) => restoreAfterFailure(sent, typed));
      setAttachments((now) => [...sentAttachments, ...now]);
      show({ tone: "error", title: res.message });
    }
  };

  const attach = (list: Attachment[] | IpcError) => {
    if ("code" in list) show({ tone: "error", title: list.message });
    else setAttachments((a) => [...a, ...list]);
  };

  const mentions = useCallback(
    async (query: string): Promise<MentionItem[]> => {
      const rows = await invoke<PathEntry[]>("thread_mention_search", { threadId: thread.id, query }).catch(() => []);
      return rows.map((r) => ({ id: r.path, label: r.path, kind: r.isDir ? "folder" : "file" }));
    },
    [thread.id],
  );

  const onMention = (item: MentionItem) => {
    const sep = thread.cwd.includes("\\") ? "\\" : "/";
    const path = `${thread.cwd.replace(/[\\/]+$/, "")}${sep}${item.id.replace(/\/$/, "")}`;
    void invoke<Attachment[]>("thread_attach_paths", { threadId: thread.id, paths: [path] }).then(attach, attach);
  };

  return (
    <>
      <Composer
        ref={composerRef}
        value={text}
        onChange={setText}
        onSend={() => void onSend()}
        onStop={() => void stop(thread.id)}
        running={running}
        placeholder={placeholder}
        sendWithModEnter={sendKey === "mod_enter"}
        onRecallLast={lastMessage}
        attachments={attachments.map((a) => ({ id: a.id, label: a.name, kind: a.kind === "tool_output" ? "file" : a.kind, error: a.error ?? undefined }))}
        onRemoveAttachment={(id) => {
          setAttachments((a) => a.filter((x) => x.id !== id));
          void invoke("attachment_remove", { attachmentId: id }).catch(() => undefined);
        }}
        queued={detail?.queued ?? null}
        onRemoveQueued={() => {
          void invoke<string | null>("thread_unqueue", { threadId: thread.id }).then((back) => {
            if (back) setText((typed) => restoreAfterFailure(back, typed));
            void reloadDetail(thread.id);
          });
        }}
        mode={thread.mode}
        onModeChange={(mode) => void update(thread.id, { mode })}
        engine={{
          value: thread.engineId,
          options: engineOptions,
          disabled: running,
          onChange: (engineId) =>
            void invoke<Thread>("thread_switch_engine", { threadId: thread.id, engineId }).then(
              () => void reloadDetail(thread.id),
              (e: IpcError) => show({ tone: "error", title: e.message }),
            ),
        }}
        model={{ value: savedModel, options: modelOptions, onChange: (model) => void update(thread.id, { model }) }}
        effort={
          efforts.length
            ? { value: thread.effort ?? efforts[Math.floor(efforts.length / 2)] ?? "", options: efforts.map((e) => ({ value: e, label: effortLabel(e) })), onChange: (effort) => void update(thread.id, { effort }) }
            : undefined
        }
        access={{
          value: thread.access,
          warning: thread.access === "full",
          options: [
            { value: "read", label: t("thread.access.read"), description: t("thread.access.read.desc") },
            { value: "edits", label: t("thread.access.edits"), description: t("thread.access.edits.desc") },
            { value: "full", label: t("thread.access.full"), description: t("thread.access.full.desc") },
          ],
          onChange: setAccess,
        }}
        tokens={{
          text: tokenCount(tokens.input + tokens.output),
          tooltip: t("thread.composer.tokens", { input: tokenCount(tokens.input), output: tokenCount(tokens.output) }),
        }}
        onAttach={() => void invoke<Attachment[]>("thread_pick_files", { threadId: thread.id }).then(attach, attach)}
        dropActive={dragOver}
        onPasteImage={() =>
          void invoke<Attachment>("thread_attach_clipboard_image", { threadId: thread.id }).then(
            (a) => attach([a]),
            // A paste with nothing usable on the clipboard does nothing.
            (e: IpcError) => e.code !== "not_found" && attach(e),
          )
        }
        mentions={mentions}
        onMention={onMention}
        labels={{
          message: t("thread.composer.message"),
          send: t("thread.composer.send"),
          stop: t("thread.composer.stop"),
          emptySend: t("thread.composer.empty"),
          attach: t("thread.composer.attach"),
          dictate: t("thread.composer.dictate"),
          mode: t("thread.composer.mode"),
          plan: t("thread.composer.plan"),
          act: t("thread.composer.act"),
          modeTooltip: t("thread.composer.modeTooltip"),
          engine: t("thread.composer.engine"),
          model: t("thread.composer.model"),
          effort: t("thread.composer.effort"),
          access: t("thread.composer.access"),
          queued: t("thread.composer.queued"),
          removeQueued: t("thread.composer.removeQueued"),
          mentions: t("thread.composer.mentions"),
          noMentions: t("thread.composer.noMentions"),
          dropToAttach: t("thread.composer.drop"),
          removeAttachment: (label) => t("thread.composer.removeAttachment", { label }),
        }}
        autoFocus={visible}
      />
      <ConfirmDialog
        open={askFull}
        onOpenChange={setAskFull}
        title={t("thread.access.fullConfirm.title")}
        body={t("thread.access.fullConfirm.body")}
        confirmLabel={t("thread.access.fullConfirm.ok")}
        onConfirm={() => {
          confirmFullAccess(thread.workspaceId ?? "none");
          setAskFull(false);
          void update(thread.id, { access: "full" });
        }}
      />
    </>
  );
}
