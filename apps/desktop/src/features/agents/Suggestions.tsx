import { useState } from "react";
import { create } from "zustand";
import { Check, X } from "lucide-react";
import type { IpcError, MemorySuggestions } from "@tandem/protocol";
import { Banner, Button, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useSteadyBox } from "../secrets/ApprovalGuard";

/** Memory suggestion sets per chat, from `memory:suggestions` (F-AG-08). */
export const useSuggestions = create<{
  byThread: Record<string, MemorySuggestions[]>;
  add: (s: MemorySuggestions) => void;
  drop: (threadId: string, setId: string) => void;
}>((set) => ({
  byThread: {},
  add: (s) =>
    set((st) => ({ byThread: { ...st.byThread, [s.threadId]: [...(st.byThread[s.threadId] ?? []), s] } })),
  drop: (threadId, setId) =>
    set((st) => ({
      byThread: {
        ...st.byThread,
        [threadId]: (st.byThread[threadId] ?? []).filter((x) => x.setId !== setId),
      },
    })),
}));

let following = false;
export function followSuggestions(): void {
  if (following) return;
  following = true;
  void listen<MemorySuggestions>("memory:suggestions", (s) => useSuggestions.getState().add(s));
}

/** Tests. */
export function resetSuggestions(): void {
  following = false;
  useSuggestions.setState({ byThread: {} });
}

function AskCard({ s }: { s: MemorySuggestions }) {
  const drop = useSuggestions((st) => st.drop);
  const { show } = useToast();
  const [decided, setDecided] = useState<Record<number, boolean>>({});
  const decide = async (i: number, keep: boolean) => {
    const next = { ...decided, [i]: keep };
    setDecided(next);
    if (Object.keys(next).length < s.items.length) return;
    const keepIdx = s.items.map((_, j) => j).filter((j) => next[j]);
    try {
      await invoke("memory_suggestions_resolve", { suggestionSetId: s.setId, keep: keepIdx });
      if (keepIdx.length) show({ tone: "success", title: t("agent.learn.kept", { n: keepIdx.length }) });
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    }
    drop(s.threadId, s.setId);
  };
  return (
    <section
      className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
      aria-label={t("agent.learn.title")}
      data-testid="memory-suggestions"
    >
      <h3 className="text-sm font-semibold uppercase tracking-caps text-fg-subtle">
        {t("agent.learn.title")}
      </h3>
      {s.rationale && <p className="text-sm text-fg-muted">{s.rationale}</p>}
      <ul className="flex flex-col gap-1">
        {s.items.map((item, i) => (
          <SuggestionRow key={i} s={s} i={i} decided={decided[i]} onDecide={(keep) => void decide(i, keep)} />
        ))}
      </ul>
    </section>
  );
}

/** One suggestion. Keep writes it into the agent's memory, so it waits like
 * Allow until its row has held still (F-TL-12 AC 16): a row decided above
 * it, a set added below or the composer growing moves it. Discard never
 * waits. */
function SuggestionRow({
  s,
  i,
  decided,
  onDecide,
}: {
  s: MemorySuggestions;
  i: number;
  decided: boolean | undefined;
  onDecide: (keep: boolean) => void;
}) {
  const item = s.items[i]!;
  const box = useSteadyBox(decided === undefined, `${s.setId}\n${i}\n${item.kind}\n${item.text}`);
  return (
    <li className="flex items-center gap-2 text-base text-fg" data-testid="memory-suggestion">
      <span className="min-w-0 flex-1">
        {item.kind === "add"
          ? t("agent.learn.add", { text: item.text })
          : t("agent.learn.remove", { text: item.text })}
      </span>
      {decided === undefined ? (
        <span ref={box.ref} className="flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            iconLeft={<Check size={14} />}
            data-grant
            data-testid="memory-suggestion-keep"
            waiting={!box.steady}
            title={box.steady ? undefined : t("approval.waitingKeep")}
            onClick={() => box.check() && onDecide(true)}
          >
            {t("agent.learn.keep")}
          </Button>
          <Button size="sm" variant="ghost" iconLeft={<X size={14} />} onClick={() => onDecide(false)}>
            {t("agent.learn.discard")}
          </Button>
        </span>
      ) : (
        <span className="text-sm text-fg-muted">
          {decided ? t("agent.learn.keeping") : t("agent.learn.discarded")}
        </span>
      )}
    </li>
  );
}

function AppliedNote({ s }: { s: MemorySuggestions }) {
  const drop = useSuggestions((st) => st.drop);
  const { show } = useToast();
  return (
    <div data-testid="memory-updated">
      <Banner
        tone="success"
        className="rounded-md"
        onDismiss={() => drop(s.threadId, s.setId)}
        dismissLabel={t("agent.learn.dismiss")}
        action={
          s.changeId
            ? {
                label: t("agent.learn.undo"),
                onClick: () =>
                  void invoke("memory_undo", { changeId: s.changeId }).then(
                    () => drop(s.threadId, s.setId),
                    (e: IpcError) => show({ tone: "error", title: e.message }),
                  ),
              }
            : undefined
        }
      >
        {t("agent.learn.updated", { n: s.items.length })}
      </Banner>
    </div>
  );
}

/** Above a chat's composer: its pending suggestions and Auto's notes. */
export function SuggestionCards({ threadId }: { threadId: string }) {
  const sets = useSuggestions((s) => s.byThread[threadId]);
  if (!sets?.length) return null;
  return (
    <div className="flex flex-col gap-2">
      {sets.map((s) => (s.applied ? <AppliedNote key={s.setId} s={s} /> : <AskCard key={s.setId} s={s} />))}
    </div>
  );
}
