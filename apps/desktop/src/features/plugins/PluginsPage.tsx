import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import type { HeldDraft, IpcError, PluginCard } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  EmptyState,
  EngineGlyph,
  Input,
  Skeleton,
  Switch,
  Tabs,
  Tooltip,
  cx,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { listen } from "../../ipc/events";
import { ConnectSheet } from "./ConnectSheet";
import { HeldActions } from "./HeldActions";
import { PluginDetailView } from "./PluginDetail";
import { CATEGORIES, filterCards, splitCards, type Category } from "./model";

/** Loads the catalog and follows `plugins:changed`. */
export function useCatalog(): { cards: PluginCard[] | null; error: string | null; reload: () => void } {
  const [cards, setCards] = useState<PluginCard[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    void invoke<PluginCard[]>("plugins_catalog").then(
      (c) => {
        setCards(c);
        setError(null);
      },
      (e: IpcError) => setError(e.message),
    );
  }, []);
  useEffect(() => {
    reload();
    let off: (() => void) | undefined;
    let gone = false;
    void listen("plugins:changed", () => reload()).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
    };
  }, [reload]);
  return { cards, error, reload };
}

/** UI-90 Settings › Plugins: the catalog, a connected plugin's page, and
 * the held actions; plus "Grant these plugins to all threads". */
export function PluginsPage() {
  const { cards, error, reload } = useCatalog();
  const [tab, setTab] = useState("catalog");
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<Category>("all");
  const [connecting, setConnecting] = useState<{ card: PluginCard; reconnect: string | null } | null>(null);
  const [detail, setDetail] = useState<string | null>(null);
  const [held, setHeld] = useState(0);
  const search = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const load = () =>
      void invoke<HeldDraft[]>("held_drafts_list", { state: "held", originId: null }).then(
        (d) => setHeld(d.length),
        () => setHeld(0),
      );
    load();
    let off: (() => void) | undefined;
    let gone = false;
    void listen("held:changed", load).then((u) => (gone ? u() : (off = u)));
    return () => {
      gone = true;
      off?.();
    };
  }, []);

  const shown = useMemo(() => filterCards(cards ?? [], query, category), [cards, query, category]);
  const { connected, browse } = splitCards(shown);

  if (detail) {
    return (
      <PluginDetailView
        connectionId={detail}
        onBack={() => setDetail(null)}
        onReconnect={(card) => setConnecting({ card, reconnect: card.connectionId ?? null })}
        connectSheet={
          connecting && (
            <ConnectSheet
              card={connecting.card}
              reconnect={connecting.reconnect}
              open
              onOpenChange={(o) => {
                if (!o) {
                  setConnecting(null);
                  reload();
                }
              }}
            />
          )
        }
      />
    );
  }

  const onKey = (e: React.KeyboardEvent) => {
    const el = e.target as HTMLElement;
    const typing = el.tagName === "INPUT" || el.tagName === "TEXTAREA";
    if (e.key === "/" && !typing) {
      e.preventDefault();
      search.current?.focus();
    } else if (!typing && /^[1-7]$/.test(e.key)) {
      setCategory(CATEGORIES[Number(e.key) - 1] ?? "all");
    }
  };

  const body = (() => {
    if (error) {
      return (
        <Banner tone="error" action={{ label: t("plugins.retry"), onClick: reload }}>
          {t("plugins.loadFailed", { error })}
        </Banner>
      );
    }
    if (!cards) {
      return (
        <div className="grid grid-cols-2 gap-3" aria-busy="true" data-testid="plugins-loading">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} height={88} />
          ))}
        </div>
      );
    }
    if (shown.length === 0) {
      return <EmptyState title={t("plugins.noMatch", { q: query })} />;
    }
    return (
      <div className="flex flex-col gap-4">
        {connected.length > 0 && (
          <Section label={t("plugins.connectedN", { n: connected.length })}>
            {connected.map((c) => (
              <Card
                key={c.connectionId ?? c.pluginId}
                card={c}
                onOpen={() => c.connectionId && setDetail(c.connectionId)}
                onConnect={() => setConnecting({ card: c, reconnect: c.connectionId ?? null })}
              />
            ))}
          </Section>
        )}
        {browse.length > 0 && (
          <Section label={t("plugins.browse")}>
            {browse.map((c) => (
              <Card
                key={c.pluginId}
                card={c}
                onOpen={() => undefined}
                onConnect={() => setConnecting({ card: c, reconnect: null })}
              />
            ))}
          </Section>
        )}
        <AllThreads cards={cards} />
        <p className="text-sm text-fg-subtle">{t("plugins.footer")}</p>
      </div>
    );
  })();

  return (
    <div className="flex flex-col gap-3" onKeyDown={onKey} data-testid="plugins-page">
      <Tabs
        label={t("plugins.tabs")}
        value={tab}
        onValueChange={setTab}
        items={[
          { id: "catalog", label: t("plugins.tab.catalog") },
          { id: "held", label: t("plugins.tab.held"), count: held || undefined },
        ]}
        renderPanel={(id) =>
          id === "held" ? (
            <div className="pt-3">
              <HeldActions />
            </div>
          ) : (
            <div className="flex flex-col gap-3 pt-3">
              <p className="text-sm text-fg-muted">{t("plugins.intro")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <div className="w-64">
                  <Input
                    ref={search}
                    aria-label={t("plugins.search")}
                    placeholder={t("plugins.search")}
                    prefix={<Search size={14} aria-hidden="true" />}
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    data-testid="plugins-search"
                  />
                </div>
                <div role="group" aria-label={t("plugins.categories")} className="flex flex-wrap gap-1">
                  {CATEGORIES.map((c) => (
                    <button
                      key={c}
                      type="button"
                      aria-pressed={category === c}
                      onClick={() => setCategory(c)}
                      className={cx(
                        "h-6 rounded-sm px-2 text-sm focus-visible:shadow-focus focus-visible:outline-none",
                        category === c ? "bg-surface-4 text-fg" : "text-fg-muted hover:bg-surface-3",
                      )}
                    >
                      {t(`plugins.category.${c}` as MessageKey)}
                    </button>
                  ))}
                </div>
              </div>
              {body}
            </div>
          )
        }
      />
      {connecting && (
        <ConnectSheet
          card={connecting.card}
          reconnect={connecting.reconnect}
          open
          onOpenChange={(o) => {
            if (!o) {
              setConnecting(null);
              reload();
            }
          }}
        />
      )}
    </div>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2" aria-label={label}>
      <h3 className="text-2xs font-medium uppercase tracking-caps text-fg-subtle">{label}</h3>
      <ul className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">{children}</ul>
    </section>
  );
}

/** One card (UI-90). */
function Card({ card, onOpen, onConnect }: { card: PluginCard; onOpen: () => void; onConnect: () => void }) {
  const preview = card.status === "preview";
  const blocked = card.state === "blocked_by_org";
  // Ready, but this build can't connect it yet (D-960).
  const unavailable = card.state === "unavailable";
  const off = preview || blocked || unavailable;
  const connected = card.state === "connected" || card.state === "needs_attention";
  const status = (() => {
    if (card.state === "connected")
      return (
        <Badge variant="done" dot>
          {t("plugins.state.connected")}
        </Badge>
      );
    if (card.state === "needs_attention")
      return (
        <Badge variant="error" dot>
          {t("plugins.state.needsAttention")}
        </Badge>
      );
    if (unavailable) return <Badge variant="outline">{t("plugins.state.unavailable")}</Badge>;
    if (preview)
      return (
        <Tooltip label={t("plugins.previewTip")}>
          <span>
            <Badge variant="outline">{t("plugins.state.preview")}</Badge>
          </span>
        </Tooltip>
      );
    return null;
  })();
  const why = preview
    ? t("plugins.previewTip")
    : blocked
      ? t("plugins.blockedByOrg", { plugin: card.label })
      : unavailable
        ? (card.unavailableReason ?? t("plugins.state.unavailable"))
        : card.limitReached
          ? t("plugins.freeLimit")
          : null;
  return (
    <li
      data-testid={`plugin-card-${card.pluginId}`}
      data-state={card.state}
      className={cx(
        "flex flex-col gap-2 rounded-md border border-border p-3",
        // Quieter, not faded: dimmed text would fail contrast.
        off ? "bg-surface-1" : "bg-surface-2",
      )}
    >
      <div className="flex items-center gap-2">
        <EngineGlyph letters={card.logo} size={20} />
        {connected ? (
          <button
            type="button"
            onClick={onOpen}
            className="min-w-0 flex-1 truncate text-left text-base font-medium text-fg hover:underline focus-visible:shadow-focus focus-visible:outline-none"
          >
            {card.label}
          </button>
        ) : (
          <span className="min-w-0 flex-1 truncate text-base font-medium text-fg">{card.label}</span>
        )}
        {status}
      </div>
      <p className="line-clamp-2 text-sm text-fg-muted">{card.description}</p>
      {connected ? (
        <div className="flex items-center gap-2 text-sm text-fg-muted">
          <span className="truncate">
            {[card.accountLabel, t("plugins.teammatesN", { n: card.teammates })].filter(Boolean).join(" · ")}
          </span>
          <span className="flex-1" />
          {card.state === "needs_attention" && (
            <Button size="sm" variant="secondary" onClick={onConnect}>
              {t("plugins.reconnect")}
            </Button>
          )}
        </div>
      ) : (
        <div className="flex items-center gap-2">
          {why && (
            <span
              className="min-w-0 flex-1 text-sm text-fg-muted"
              data-testid={`plugin-why-${card.pluginId}`}
            >
              {why}
            </span>
          )}
          {!why && <span className="flex-1" />}
          <Button
            size="sm"
            variant="secondary"
            disabled={off || card.limitReached}
            onClick={onConnect}
            data-testid={`plugin-connect-${card.pluginId}`}
            aria-label={t("plugins.connectTo", { plugin: card.label })}
          >
            {card.limitReached && !off ? t("plugins.pro") : t("plugins.connect")}
          </Button>
        </div>
      )}
    </li>
  );
}

/** "Grant these plugins to all threads" (F-PL-05): connected plugins every
 * thread gets, on top of each thread's own. */
function AllThreads({ cards }: { cards: PluginCard[] }) {
  const connected = cards.filter((c) => c.state === "connected" && c.connectionId);
  const [granted, setGranted] = useState<string[]>([]);
  useEffect(() => {
    void invoke<string[]>("grants_list", { subject: { type: "all_threads" } }).then(setGranted, () =>
      setGranted([]),
    );
  }, [cards]);
  if (connected.length === 0) return null;
  return (
    <section className="flex flex-col gap-1" aria-label={t("plugins.allThreads")}>
      <h3 className="text-base font-medium text-fg">{t("plugins.allThreads")}</h3>
      <p className="text-sm text-fg-muted">{t("plugins.allThreads.desc")}</p>
      {connected.map((c) => (
        <Switch
          key={c.connectionId}
          label={c.label}
          checked={granted.includes(c.connectionId ?? "")}
          onCheckedChange={(v) => {
            const id = c.connectionId ?? "";
            setGranted((g) => (v ? [...g, id] : g.filter((x) => x !== id)));
            void invoke("grants_set", {
              subject: { type: "all_threads" },
              connectionId: id,
              granted: v,
            }).catch(() => undefined);
          }}
        />
      ))}
    </section>
  );
}
