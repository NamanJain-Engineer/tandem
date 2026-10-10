// The Files editor (F-CD-19): open-file tabs (closing one selects its left
// neighbour), CodeMirror for text, a hex placeholder for binary files, a
// preview for images, read-only banners, and the "File changed on disk"
// dialog with Reload / Overwrite / Compare.
import { useEffect, useState, type MouseEvent } from "react";
import { ChevronLeft, FileWarning, X } from "lucide-react";
import type { FileDiff, IpcError } from "@tandem/protocol";
import { Banner, Button, CodeEditor, Dialog, DiffView, EmptyState, IconButton, Skeleton, Spinner, cx } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { useFiles, type OpenFile } from "../../stores/files";

const READONLY: Record<string, MessageKey> = {
  outside: "files.readonly.outside",
  large: "files.readonly.large",
  permission: "files.readonly.permission",
};

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function Tab({ file, active }: { file: OpenFile; active: boolean }) {
  const name = baseName(file.path);
  const close = (e: MouseEvent) => {
    e.stopPropagation();
    useFiles.getState().close(file.path);
  };
  return (
    <div
      role="presentation"
      className={cx(
        "group flex h-7 shrink-0 items-center gap-1 rounded-t-sm border-b-2 pl-2.5 pr-1 text-sm",
        active ? "border-accent bg-surface-2 text-fg" : "border-transparent text-fg-muted hover:bg-surface-2",
      )}
    >
      <button
        type="button"
        role="tab"
        aria-selected={active}
        title={file.path}
        data-testid="editor-tab"
        data-path={file.path}
        data-dirty={file.dirty || undefined}
        aria-label={file.dirty ? t("files.tabs.unsaved", { name }) : name}
        aria-keyshortcuts="Delete"
        tabIndex={active ? 0 : -1}
        onClick={() => useFiles.getState().activate(file.path)}
        onAuxClick={(e) => e.button === 1 && close(e)}
        onKeyDown={(e) => {
          if (e.key === "Delete") {
            e.preventDefault();
            useFiles.getState().close(file.path);
          }
        }}
        className="max-w-[160px] truncate outline-none focus-visible:shadow-focus"
      >
        {name}
      </button>
      {/* A tab list may only hold tabs: × is for the mouse; the keyboard
          closes the focused tab with Delete (aria-keyshortcuts). */}
      <span
        aria-hidden="true"
        title={t("files.tabs.close", { name })}
        data-testid="editor-tab-close"
        onClick={close}
        className="relative inline-flex size-4 items-center justify-center rounded-full text-fg-subtle hover:bg-surface-4 hover:text-fg"
      >
        {file.dirty ? <span className="size-2 rounded-full bg-fg-muted group-hover:hidden" /> : null}
        <X size={11} className={cx(file.dirty && "hidden group-hover:block")} />
      </span>
    </div>
  );
}

function ConflictDialog({ file }: { file: OpenFile }) {
  const [diff, setDiff] = useState<FileDiff | IpcError | null>(null);
  const [busy, setBusy] = useState(false);
  const name = baseName(file.path);
  const store = useFiles.getState;
  const close = () => {
    setDiff(null);
    useFiles.setState((s) => {
      const key = s.key;
      const c = key ? s.byKey[key] : undefined;
      if (!key || !c) return {};
      return { byKey: { ...s.byKey, [key]: { ...c, tabs: c.tabs.map((f) => (f.path === file.path ? { ...f, conflict: false } : f)) } } };
    });
  };
  const comparing = diff && !("code" in diff);
  return (
    <Dialog
      open={file.conflict}
      onOpenChange={(o) => !o && close()}
      title={comparing ? t("files.compare.title", { name }) : t("files.conflict.title")}
      description={comparing ? undefined : t("files.conflict.body", { name })}
      size={comparing ? "lg" : "md"}
      footer={
        <div className="flex w-full items-center gap-2" data-testid="conflict-dialog">
          {comparing ? (
            <Button variant="ghost" onClick={() => setDiff(null)}>
              {t("files.conflict.back")}
            </Button>
          ) : (
            <Button
              variant="ghost"
              loading={busy && diff === null}
              onClick={async () => {
                setBusy(true);
                setDiff(await store().compare(file.path));
                setBusy(false);
              }}
            >
              {t("files.conflict.compare")}
            </Button>
          )}
          <span className="flex-1" />
          <Button variant="secondary" onClick={() => void store().reload(file.path)}>
            {t("files.conflict.reload")}
          </Button>
          <Button variant="primary" onClick={() => void store().overwrite(file.path)}>
            {t("files.conflict.overwrite")}
          </Button>
        </div>
      }
    >
      {diff && "code" in diff && <Banner tone="error">{diff.message}</Banner>}
      {comparing && (
        <div className="max-h-[50vh] overflow-auto" data-testid="conflict-diff">
          <DiffView files={[diff]} labels={{ empty: t("files.compare.same") }} />
        </div>
      )}
    </Dialog>
  );
}

function Body({ file }: { file: OpenFile }) {
  const c = file.content;
  if (file.status === "loading") {
    return (
      <div role="status" aria-busy="true" aria-label={t("files.loading")} className="flex flex-col gap-2 p-4">
        <Skeleton width="60%" />
        <Skeleton width="80%" />
        <Skeleton width="45%" />
      </div>
    );
  }
  if (file.status === "error" || !c) {
    return (
      <EmptyState
        icon={<FileWarning size={18} />}
        title={t("files.error.title", { name: baseName(file.path) })}
        body={file.error?.message}
        primary={
          <Button size="sm" onClick={() => void useFiles.getState().reload(file.path)}>
            {t("files.retry")}
          </Button>
        }
      />
    );
  }
  if (c.kind === "image") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 overflow-auto p-4">
        {c.dataUrl ? (
          <img src={c.dataUrl} alt={baseName(file.path)} className="max-h-full max-w-full rounded-sm border border-border object-contain" />
        ) : (
          <p className="text-sm text-fg-muted">{t("files.imageTooLarge", { size: formatSize(c.size) })}</p>
        )}
        <p className="text-2xs text-fg-subtle">{formatSize(c.size)}</p>
      </div>
    );
  }
  if (c.kind === "binary") {
    return (
      <div className="flex h-full flex-col gap-2 overflow-auto p-3" data-testid="binary-placeholder">
        <p className="text-sm text-fg-muted">{t("files.binary", { size: formatSize(c.size) })}</p>
        <pre className="font-mono text-2xs leading-relaxed text-fg-subtle">{c.hex}</pre>
      </div>
    );
  }
  const label = c.readonly ? t("files.editor.labelReadOnly", { path: file.path }) : t("files.editor.label", { path: file.path });
  return (
    <CodeEditor
      docKey={file.path}
      path={file.path}
      label={label}
      value={file.text}
      readOnly={c.readonly}
      goto={file.goto}
      onChange={(text) => useFiles.getState().edit(file.path, text)}
      onSave={() => void useFiles.getState().save(file.path)}
    />
  );
}

/** The editor column: tabs, banners and the open file. */
export function FileEditor({ onBack }: { onBack?: () => void }) {
  const ctx = useFiles((s) => (s.key ? s.byKey[s.key] : undefined));
  const tabs = ctx?.tabs ?? [];
  const active = tabs.find((f) => f.path === ctx?.active) ?? null;

  // The narrow layout goes back to the tree when the last tab closes.
  useEffect(() => {
    if (onBack && tabs.length === 0) onBack();
  }, [onBack, tabs.length]);

  return (
    <section aria-label={t("files.editor.region")} className="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="file-editor">
      <div className="flex h-8 shrink-0 items-end gap-0.5 overflow-x-auto border-b border-divider px-1">
        {onBack && (
          <IconButton
            size="sm"
            icon={<ChevronLeft size={14} />}
            label={t("files.back")}
            onClick={onBack}
            data-testid="files-back"
            className="mb-0.5"
          />
        )}
        <div role="tablist" aria-label={t("files.tabs.label")} className="flex min-w-0 items-end gap-0.5">
          {tabs.map((f) => (
            <Tab key={f.path} file={f} active={f.path === active?.path} />
          ))}
        </div>
        {active?.saving && (
          <span className="mb-1.5 ml-auto flex items-center gap-1 pr-1 text-2xs text-fg-subtle" role="status">
            <Spinner size={10} />
            {t("files.saving")}
          </span>
        )}
      </div>
      {active?.content?.readonlyReason && (
        <Banner tone={active.content.readonlyReason === "large" ? "warning" : "info"}>
          {t(READONLY[active.content.readonlyReason] ?? "files.readonly.outside")}
        </Banner>
      )}
      {active?.content?.truncated && <Banner tone="warning">{t("files.truncated")}</Banner>}
      {active?.changedOnDisk && !active.conflict && (
        <Banner tone="warning" action={{ label: t("files.conflict.reload"), onClick: () => void useFiles.getState().reload(active.path) }}>
          {t("files.changedOnDisk")}
        </Banner>
      )}
      {active?.error && active.status === "ready" && (
        <Banner tone="error" action={{ label: t("files.retry"), onClick: () => void useFiles.getState().save(active.path) }}>
          {t("files.saveFailed", { name: baseName(active.path), message: active.error.message })}
        </Banner>
      )}
      <div className="relative min-h-0 flex-1" role="tabpanel" aria-label={active ? baseName(active.path) : t("files.tabs.label")}>
        {active ? (
          <Body file={active} />
        ) : (
          <EmptyState title={t("files.editor.empty.title")} body={t("files.editor.empty.body")} />
        )}
      </div>
      {active?.conflict && <ConflictDialog file={active} />}
    </section>
  );
}
