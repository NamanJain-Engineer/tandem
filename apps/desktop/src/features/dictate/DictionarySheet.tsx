import { useEffect, useMemo, useState } from "react";
import type { ImportPreview, IpcError, Rule, Suggestion } from "@tandem/protocol";
import {
  Button,
  Checkbox,
  Dialog,
  EmptyState,
  Input,
  SegmentedControl,
  Select,
  Sheet,
  Spinner,
  Switch,
  Tabs,
  Textarea,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

type Tab = "words" | "corrections" | "review" | "deleted";

export function emptyRule(kind: Rule["kind"]): Rule {
  return {
    id: "",
    kind,
    replacement: "",
    phrases: [],
    matchCase: false,
    protect: true,
    spacing: "exact",
    enabled: true,
    lang: "*",
    appProfiles: [],
    updatedAt: 0,
    deletedAt: null,
  };
}

function RuleEditor({ rule, onClose, onSaved }: { rule: Rule; onClose: () => void; onSaved: () => void }) {
  const [draft, setDraft] = useState(rule);
  const [phrases, setPhrases] = useState(rule.phrases.join("\n"));
  const [apps, setApps] = useState(rule.appProfiles.join("\n"));
  const [error, setError] = useState<string | null>(null);
  const word = draft.kind === "word";
  const save = () =>
    invoke<Rule>("dictionary_save", {
      rule: {
        ...draft,
        phrases: phrases.split("\n"),
        appProfiles: apps.split("\n"),
      },
    }).then(
      () => {
        onSaved();
        onClose();
      },
      (e: IpcError) => setError(e.message),
    );
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={word ? t("dictionary.editor.word") : t("dictionary.editor.correction")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("dictionary.cancel")}
          </Button>
          <Button variant="primary" onClick={() => void save()} data-testid="rule-save">
            {t("dictionary.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3" data-testid="rule-editor">
        <Textarea
          label={word ? t("dictionary.field.misheard") : t("dictionary.field.when")}
          value={phrases}
          rows={3}
          onChange={(e) => setPhrases(e.target.value)}
          data-testid="rule-phrases"
        />
        <Input
          label={word ? t("dictionary.field.spelling") : t("dictionary.field.write")}
          value={draft.replacement}
          onChange={(e) => setDraft({ ...draft, replacement: e.target.value })}
          data-testid="rule-replacement"
        />
        <Checkbox
          label={t("dictionary.field.matchCase")}
          checked={draft.matchCase}
          onCheckedChange={(v) => setDraft({ ...draft, matchCase: v })}
        />
        <Checkbox
          label={t("dictionary.field.protect")}
          checked={draft.protect}
          onCheckedChange={(v) => setDraft({ ...draft, protect: v })}
        />
        <SegmentedControl
          label={t("dictionary.field.spacing")}
          size="sm"
          value={draft.spacing}
          onValueChange={(v) => setDraft({ ...draft, spacing: v === "flexible" ? "flexible" : "exact" })}
          items={[
            { id: "exact", label: t("dictionary.spacing.exact") },
            { id: "flexible", label: t("dictionary.spacing.flexible") },
          ]}
        />
        <Input
          label={t("dictionary.field.language")}
          value={draft.lang === "*" ? "" : draft.lang}
          placeholder={t("dictionary.field.languageAll")}
          onChange={(e) => setDraft({ ...draft, lang: e.target.value.trim() || "*" })}
        />
        <Textarea
          label={t("dictionary.field.apps")}
          value={apps}
          rows={2}
          placeholder={t("dictionary.field.appsEverywhere")}
          onChange={(e) => setApps(e.target.value)}
          data-testid="rule-apps"
        />
        {error && (
          <p role="alert" className="text-sm text-[var(--tn-danger)]">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}

function RuleList({
  rules,
  kind,
  onChanged,
}: {
  rules: Rule[];
  kind: Rule["kind"];
  onChanged: () => void;
}) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<"az" | "recent">("az");
  const [picked, setPicked] = useState<string[]>([]);
  const [editing, setEditing] = useState<Rule | null>(null);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = rules
      .filter((r) => r.kind === kind)
      .filter((r) => !q || [r.replacement, ...r.phrases].some((s) => s.toLowerCase().includes(q)));
    return sort === "az"
      ? list.sort((a, b) => a.replacement.localeCompare(b.replacement))
      : list.sort((a, b) => Number(b.updatedAt) - Number(a.updatedAt));
  }, [rules, kind, query, sort]);
  const toggle = (r: Rule, enabled: boolean) =>
    void invoke("dictionary_save", { rule: { ...r, enabled } }).then(onChanged, () => undefined);
  const remove = () =>
    void invoke("dictionary_delete", { ids: picked }).then(() => {
      setPicked([]);
      onChanged();
    });
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <Input label={t("dictionary.search")} hideLabel placeholder={t("dictionary.search")} value={query} onChange={(e) => setQuery(e.target.value)} className="flex-1" />
        <Select
          ariaLabel={t("dictionary.sort")}
          size="sm"
          value={sort}
          onValueChange={(v) => setSort(v === "recent" ? "recent" : "az")}
          options={[
            { value: "az", label: t("dictionary.sort.az") },
            { value: "recent", label: t("dictionary.sort.recent") },
          ]}
          className="w-40"
        />
        {picked.length > 0 && (
          <Button size="sm" variant="danger" onClick={remove} data-testid="rules-delete">
            {t("dictionary.delete", { n: picked.length })}
          </Button>
        )}
        <Button size="sm" variant="primary" onClick={() => setEditing(emptyRule(kind))} data-testid={`rules-add-${kind}`}>
          {kind === "word" ? t("dictionary.addWord") : t("dictionary.addCorrection")}
        </Button>
      </div>
      {shown.length === 0 ? (
        <EmptyState
          title={kind === "word" ? t("dictionary.empty.title") : t("dictionary.empty.corrections")}
          body={t("dictionary.empty.body")}
        />
      ) : (
        <ul className="flex flex-col divide-y divide-divider rounded-md border border-border" data-testid={`rules-${kind}`}>
          {shown.map((r) => (
            <li key={r.id} className="flex items-center gap-3 px-3 py-2" data-rule={r.id}>
              <Checkbox
                label={<span className="sr-only">{t("dictionary.pick", { name: r.replacement })}</span>}
                checked={picked.includes(r.id)}
                onCheckedChange={(v) => setPicked((p) => (v ? [...p, r.id] : p.filter((x) => x !== r.id)))}
              />
              <button
                type="button"
                className="flex min-w-0 flex-1 flex-col text-left focus-visible:shadow-focus focus-visible:outline-none"
                onClick={() => setEditing(r)}
              >
                <span className="truncate text-base text-fg">
                  {kind === "word" ? r.replacement : `${r.phrases.join(", ")} → ${r.replacement}`}
                </span>
                <span className="truncate text-sm text-fg-muted">
                  {kind === "word" && r.phrases.length > 0 ? t("dictionary.misheardAs", { list: r.phrases.join(", ") }) : null}
                  {r.appProfiles.length > 0 ? ` ${t("dictionary.onlyIn", { apps: r.appProfiles.join(", ") })}` : null}
                </span>
              </button>
              <Switch checked={r.enabled} aria-label={t("dictionary.enabled", { name: r.replacement })} onCheckedChange={(v) => toggle(r, v)} />
            </li>
          ))}
        </ul>
      )}
      {editing && <RuleEditor rule={editing} onClose={() => setEditing(null)} onSaved={onChanged} />}
    </div>
  );
}

function Review({ onChanged }: { onChanged: () => void }) {
  const [items, setItems] = useState<Suggestion[] | null>(null);
  const load = () => void invoke<Suggestion[] | null>("dictionary_suggestions").then((s) => setItems(s ?? []), () => setItems([]));
  useEffect(load, []);
  const resolve = (id: string, remember: boolean) =>
    void invoke("dictionary_suggestion_resolve", { id, remember }).then(() => {
      load();
      onChanged();
    });
  if (!items) return <Spinner label={t("dictate.loading")} />;
  if (items.length === 0) return <EmptyState title={t("dictionary.review.empty")} body={t("dictionary.review.emptyBody")} />;
  return (
    <ul className="flex flex-col divide-y divide-divider rounded-md border border-border" data-testid="suggestions">
      {items.map((s) => (
        <li key={s.id} className="flex items-center gap-3 px-3 py-2">
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-base text-fg">
              {s.term} <span className="text-sm text-fg-muted">· {t(`dictionary.kind.${s.kind}` as Parameters<typeof t>[0])}</span>
            </span>
            <span className="truncate text-sm text-fg-muted">{s.evidence.map((e) => `“${e}”`).join("  ")}</span>
          </div>
          <Button size="sm" variant="ghost" onClick={() => resolve(s.id, false)}>
            {t("dictionary.review.ignore")}
          </Button>
          <Button size="sm" variant="secondary" onClick={() => resolve(s.id, true)}>
            {t("dictionary.review.remember")}
          </Button>
        </li>
      ))}
    </ul>
  );
}

function Deleted({ onChanged }: { onChanged: () => void }) {
  const [items, setItems] = useState<Rule[] | null>(null);
  const load = () => void invoke<Rule[] | null>("dictionary_deleted").then((r) => setItems(r ?? []), () => setItems([]));
  useEffect(load, []);
  if (!items) return <Spinner label={t("dictate.loading")} />;
  if (items.length === 0) return <EmptyState title={t("dictionary.deleted.empty")} body={t("dictionary.deleted.body")} />;
  return (
    <ul className="flex flex-col divide-y divide-divider rounded-md border border-border">
      {items.map((r) => (
        <li key={r.id} className="flex items-center gap-3 px-3 py-2">
          <span className="flex-1 truncate text-base text-fg">{r.replacement}</span>
          <Button
            size="sm"
            variant="secondary"
            onClick={() =>
              void invoke("dictionary_restore", { id: r.id }).then(() => {
                load();
                onChanged();
              })
            }
          >
            {t("dictionary.deleted.restore")}
          </Button>
        </li>
      ))}
    </ul>
  );
}

/** "Try your dictionary on sample text": live before/after. */
function TestBox() {
  const [sample, setSample] = useState("");
  const [app, setApp] = useState("");
  const [out, setOut] = useState("");
  useEffect(() => {
    const id = setTimeout(() => {
      if (!sample.trim()) return setOut("");
      void invoke<string>("dictionary_test", { sample, appId: app || null, lang: null }).then(setOut, () => undefined);
    }, 150);
    return () => clearTimeout(id);
  }, [sample, app]);
  return (
    <section className="flex flex-col gap-2 rounded-md border border-border p-3" aria-label={t("dictionary.test.title")}>
      <h3 className="text-sm font-semibold text-fg">{t("dictionary.test.title")}</h3>
      <div className="flex gap-2">
        <Input label={t("dictionary.test.sample")} hideLabel placeholder={t("dictionary.test.sample")} value={sample} onChange={(e) => setSample(e.target.value)} className="flex-1" data-testid="dictionary-test-input" />
        <Input label={t("dictionary.test.app")} hideLabel placeholder={t("dictionary.test.app")} value={app} onChange={(e) => setApp(e.target.value)} className="w-48" data-testid="dictionary-test-app" />
      </div>
      <p className="min-h-5 text-sm text-fg" data-testid="dictionary-test-output">
        {out}
      </p>
    </section>
  );
}

function ImportExport({ onChanged }: { onChanged: () => void }) {
  const { show } = useToast();
  const [format, setFormat] = useState<"json" | "csv">("json");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const doImport = async () => {
    const path = await invoke<string | null>("dictate_pick_file", { kind: format }, { timeoutMs: 24 * 3_600_000 });
    if (!path) return;
    await invoke<ImportPreview>("dictionary_import_preview", { path, format }).then(setPreview, (e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  };
  const doExport = async () => {
    const path = await invoke<string | null>(
      "dictate_save_file",
      { suggestedName: `vibezeo-dictionary.${format}`, kind: format },
      { timeoutMs: 24 * 3_600_000 },
    );
    if (!path) return;
    await invoke("dictionary_export", { path, format }).then(
      () => show({ tone: "success", title: t("dictionary.exported") }),
      (e: IpcError) => show({ tone: "error", title: e.message }),
    );
  };
  return (
    <div className="flex items-center gap-2">
      <SegmentedControl
        label={t("dictionary.format")}
        size="sm"
        value={format}
        onValueChange={(v) => setFormat(v === "csv" ? "csv" : "json")}
        items={[
          { id: "json", label: "JSON" },
          { id: "csv", label: "CSV" },
        ]}
      />
      <Button size="sm" variant="secondary" onClick={() => void doImport()} data-testid="dictionary-import">
        {t("dictionary.import")}
      </Button>
      <Button size="sm" variant="secondary" onClick={() => void doExport()} data-testid="dictionary-export">
        {t("dictionary.export")}
      </Button>
      {preview && (
        <Dialog
          open
          onOpenChange={(o) => !o && setPreview(null)}
          title={t("dictionary.preview.title")}
          footer={
            <>
              <Button variant="ghost" onClick={() => setPreview(null)}>
                {t("dictionary.cancel")}
              </Button>
              <Button
                variant="primary"
                disabled={preview.added.length + preview.updated.length === 0}
                onClick={() =>
                  void invoke<number>("dictionary_import_commit", { previewId: preview.previewId }).then((n) => {
                    setPreview(null);
                    onChanged();
                    show({ tone: "success", title: t("dictionary.imported", { n }) });
                  })
                }
                data-testid="dictionary-import-commit"
              >
                {t("dictionary.preview.import")}
              </Button>
            </>
          }
        >
          <p className="text-sm text-fg" data-testid="dictionary-preview">
            {t("dictionary.preview.summary", {
              added: preview.added.length,
              updated: preview.updated.length,
              skipped: preview.skipped.length,
            })}
          </p>
          <ul className="mt-2 flex max-h-48 flex-col gap-1 overflow-y-auto text-sm text-fg-muted">
            {preview.skipped.map((s) => (
              <li key={s.line}>{t("dictionary.preview.skip", { line: s.line, reason: s.reason })}</li>
            ))}
          </ul>
        </Dialog>
      )}
    </div>
  );
}

/** UI-64: Words, Corrections, Review and Deleted, with the test box and
 * import/export. Everything stays on this device. */
export function DictionarySheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [rules, setRules] = useState<Rule[] | null>(null);
  const [tab, setTab] = useState<Tab>("words");
  const [reviewCount, setReviewCount] = useState(0);
  const load = () => {
    void invoke<Rule[] | null>("dictionary_list").then((r) => setRules(r ?? []), () => setRules([]));
    void invoke<Suggestion[] | null>("dictionary_suggestions").then((s) => setReviewCount(s?.length ?? 0), () => undefined);
  };
  useEffect(() => {
    if (open) load();
  }, [open]);
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()} title={t("dictionary.title")} size="lg">
      {!rules ? (
        <Spinner label={t("dictate.loading")} />
      ) : (
        <div className="flex flex-col gap-4" data-testid="dictionary-sheet">
          <Tabs
            label={t("dictionary.title")}
            value={tab}
            onValueChange={(v) => setTab(v as Tab)}
            items={[
              { id: "words", label: t("dictionary.tab.words") },
              { id: "corrections", label: t("dictionary.tab.corrections") },
              { id: "review", label: t("dictionary.tab.review"), count: reviewCount },
              { id: "deleted", label: t("dictionary.tab.deleted") },
            ]}
            renderPanel={(id) =>
              id === "review" ? (
                <Review onChanged={load} />
              ) : id === "deleted" ? (
                <Deleted onChanged={load} />
              ) : (
                <RuleList rules={rules} kind={id === "words" ? "word" : "correction"} onChanged={load} />
              )
            }
          />
          <TestBox />
          <ImportExport onChanged={load} />
        </div>
      )}
    </Sheet>
  );
}
