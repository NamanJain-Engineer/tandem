import { useCallback, useEffect, useMemo, useState } from "react";
import { History } from "lucide-react";
import type { LibraryListing, LibrarySession, TranscriptItem } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  Dialog,
  EmptyState,
  EngineGlyph,
  Input,
  Skeleton,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { showOpened, useHandoff } from "../../stores/handoff";
import {
  groups,
  listLibrary,
  readTranscript,
  repoName,
  resumeSession,
  searchLibrary,
  useLibrary,
} from "../../stores/library";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { ago } from "../notifications/NotificationsPopover";
import { formatSize } from "../files/FileEditor";

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; listing: LibraryListing };

function matches(s: LibrarySession, q: string): boolean {
  const needle = q.trim().toLowerCase();
  if (!needle) return true;
  return [s.title, s.folder].some((v) => v?.toLowerCase().includes(needle));
}

function Transcript({ session, onBack }: { session: LibrarySession; onBack: () => void }) {
  const [items, setItems] = useState<TranscriptItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void readTranscript(session.key).then((out) => {
      if (!live) return;
      if (out.ok) setItems(out.value);
      else setError(out.error.message);
    });
    return () => {
      live = false;
    };
  }, [session.key]);
  return (
    <section className="flex flex-col gap-2" data-testid="library-transcript">
      <div className="flex items-center gap-2">
        <Button size="sm" variant="ghost" onClick={onBack}>
          {t("library.back")}
        </Button>
        <span className="truncate text-sm text-fg-muted">{session.title ?? t("library.untitled")}</span>
      </div>
      {error ? (
        <Banner tone="error">{error}</Banner>
      ) : items === null ? (
        <div className="flex flex-col gap-2" aria-busy="true" aria-label={t("library.loading")}>
          <Skeleton height={40} />
          <Skeleton height={40} />
        </div>
      ) : items.length === 0 ? (
        <p className="text-sm text-fg-muted">{t("library.transcriptEmpty")}</p>
      ) : (
        <ol
          className="flex max-h-[60vh] flex-col gap-2 overflow-y-auto"
          role="log"
          aria-label={t("library.transcript")}
        >
          {items.map((item, i) =>
            item.role === "tool" ? (
              <li key={i} className="text-xs text-fg-subtle" data-role="tool">
                {t("library.usedTool", { name: item.text })}
              </li>
            ) : (
              <li
                key={i}
                data-role={item.role}
                data-testid="library-message"
                className={
                  item.role === "user"
                    ? "rounded-md bg-surface-3 p-2 text-sm text-fg"
                    : "whitespace-pre-wrap p-2 text-sm text-fg-muted"
                }
              >
                <span className="mb-1 block text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
                  {item.role === "user" ? t("library.you") : t("library.engine")}
                </span>
                <span className="whitespace-pre-wrap">{item.text}</span>
              </li>
            ),
          )}
        </ol>
      )}
    </section>
  );
}

function Row({
  s,
  now,
  onOpen,
  onResume,
  onContinue,
  resuming,
}: {
  s: LibrarySession;
  now: number;
  onOpen: () => void;
  onResume: () => void;
  onContinue: () => void;
  resuming: boolean;
}) {
  const engine = useEngines((st) => st.list.find((e) => e.spec.id === s.engineId));
  const installed = engine?.install !== null && engine !== undefined;
  const why = !s.folderExists ? t("library.folderGone") : !installed ? t("library.notInstalled") : undefined;
  return (
    <li
      data-testid="library-row"
      data-key={s.key}
      data-session-id={s.sessionId}
      className="flex items-center gap-2 border-b border-divider px-2 py-2 last:border-b-0"
    >
      <EngineGlyph letters={engine?.spec.logo ?? s.engineId.slice(0, 2)} size={20} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-base text-fg">{s.title ?? t("library.untitled")}</span>
        <span className="truncate text-xs text-fg-muted">
          {[
            engine?.spec.label ?? s.engineId,
            s.accountLabel ?? t("library.outside"),
            ago(s.modifiedAt, now),
            formatSize(s.size),
          ].join(" · ")}
        </span>
      </span>
      <Button size="sm" variant="ghost" onClick={onOpen}>
        {t("library.open")}
      </Button>
      <Button
        size="sm"
        variant="secondary"
        disabled={why !== undefined}
        title={why}
        loading={resuming}
        onClick={onResume}
        data-testid="library-resume"
      >
        {t("library.resume")}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={!s.folderExists}
        title={s.folderExists ? undefined : t("library.folderGone")}
        onClick={onContinue}
        data-testid="library-continue"
      >
        {t("library.continue")}
      </Button>
    </li>
  );
}

function LibraryBody() {
  const hide = () => useLibrary.getState().hide();
  const { show } = useToast();
  const pro = useSession((s) => s.entitlement?.limits.sessionSearch ?? false);
  const searchOn = useSettings((s) => s.values.session_search === true);
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<string[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [open, setOpen] = useState<LibrarySession | null>(null);
  const [resuming, setResuming] = useState<string | null>(null);
  const now = Date.now();

  const fetch = useCallback(() => {
    setLoad({ at: "loading" });
    void listLibrary().then((out) =>
      setLoad(out.ok ? { at: "ready", listing: out.value } : { at: "error", message: out.error.message }),
    );
  }, []);
  useEffect(() => {
    fetch();
    if (!useEngines.getState().loaded) void useEngines.getState().refresh();
  }, [fetch]);

  const shown = useMemo(() => {
    if (load.at !== "ready") return [];
    const all = load.listing.sessions;
    if (hits) {
      const byKey = new Map(all.map((s) => [s.key, s]));
      return hits.flatMap((k) => byKey.get(k) ?? []);
    }
    return all.filter((s) => matches(s, query));
  }, [load, hits, query]);

  const fullText = async () => {
    setSearching(true);
    const out = await searchLibrary(query);
    setSearching(false);
    if (out.ok) setHits(out.value);
    else show({ tone: "error", title: out.error.message });
  };

  const resume = async (s: LibrarySession) => {
    setResuming(s.key);
    const out = await resumeSession(s.key);
    setResuming(null);
    if (!out.ok) return show({ tone: "error", title: out.error.message });
    await showOpened(out.value);
    hide();
  };

  const continueWith = (s: LibrarySession) => {
    hide();
    useHandoff.getState().open({
      paneId: "",
      workspaceId: "",
      kind: "agent",
      engineId: s.engineId,
      libraryKey: s.key,
    });
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && hide()}
      title={t("library.title")}
      description={t("library.description")}
      closeLabel={t("common.close")}
      size="lg"
      footer={
        <Button variant="secondary" onClick={hide}>
          {t("common.close")}
        </Button>
      }
    >
      <div className="flex flex-col gap-3" data-testid="library">
        {open ? (
          <Transcript session={open} onBack={() => setOpen(null)} />
        ) : load.at === "loading" ? (
          <div className="flex flex-col gap-2" aria-busy="true" aria-label={t("library.loading")}>
            <Skeleton height={32} />
            <Skeleton height={48} />
            <Skeleton height={48} />
          </div>
        ) : load.at === "error" ? (
          <Banner tone="error" action={{ label: t("library.retry"), onClick: fetch }}>
            {t("library.loadFailed", { reason: load.message })}
          </Banner>
        ) : load.listing.sessions.length === 0 ? (
          <>
            <EmptyState
              icon={<History size={20} />}
              title={t("library.emptyTitle")}
              body={t("library.emptyBody")}
            />
            {load.listing.skipped > 0 && (
              <p className="text-sm text-fg-muted" data-testid="library-skipped">
                {t("library.skipped", { n: load.listing.skipped })}
              </p>
            )}
          </>
        ) : (
          <>
            <div className="flex items-end gap-2">
              <Input
                label={t("library.filter")}
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value);
                  setHits(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && pro && searchOn && query.trim()) void fullText();
                }}
                className="flex-1"
                data-testid="library-filter"
              />
              {pro && searchOn && (
                <Button
                  variant="secondary"
                  disabled={!query.trim()}
                  loading={searching}
                  onClick={() => void fullText()}
                  data-testid="library-search"
                >
                  {t("library.search")}
                </Button>
              )}
            </div>
            {!pro ? (
              <p className="flex items-center gap-2 text-xs text-fg-subtle">
                <Badge variant="accent">{t("library.pro")}</Badge>
                {t("library.searchPro")}
              </p>
            ) : (
              !searchOn && <p className="text-xs text-fg-subtle">{t("library.searchOff")}</p>
            )}
            {load.listing.skipped > 0 && (
              <p className="text-sm text-fg-muted" data-testid="library-skipped">
                {t("library.skipped", { n: load.listing.skipped })}
              </p>
            )}
            {shown.length === 0 ? (
              <p className="text-sm text-fg-muted" data-testid="library-no-match">
                {t("library.noMatch")}
              </p>
            ) : (
              <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto">
                {groups(shown).map((g) => (
                  <section
                    key={g.repository ?? ""}
                    className="flex flex-col gap-1"
                    data-testid="library-group"
                  >
                    <h3
                      className="truncate text-2xs font-semibold uppercase tracking-caps text-fg-subtle"
                      title={g.repository ?? undefined}
                    >
                      {g.repository ? repoName(g.repository) : t("library.noFolder")}
                    </h3>
                    <ul className="rounded-md border border-border bg-surface-2">
                      {g.sessions.map((s) => (
                        <Row
                          key={s.key}
                          s={s}
                          now={now}
                          resuming={resuming === s.key}
                          onOpen={() => setOpen(s)}
                          onResume={() => void resume(s)}
                          onContinue={() => continueWith(s)}
                        />
                      ))}
                    </ul>
                  </section>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </Dialog>
  );
}

/** UI-162 Session library (F-TH-17), from the command palette. */
export function SessionLibrary() {
  const open = useLibrary((s) => s.open);
  return open ? <LibraryBody /> : null;
}
