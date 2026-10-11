import { useCallback, useEffect, useState } from "react";
import { Check, X } from "lucide-react";
import type { CallLogEntry } from "@tandem/protocol";
import { Banner, Button, EmptyState, Select, Sheet, Skeleton, SkeletonGate, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { callLog, useConductor } from "../../stores/conductor";
import { useShortcutKeys } from "../commands/useShortcutKeys";
import { clock } from "./ring";

function when(ms: number): string {
  return new Date(ms).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

function time(ms: number): string {
  return new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", second: "2-digit" });
}

/** The log as plain text, for "Copy log". */
export function logText(entry: CallLogEntry, name: string): string {
  return entry.events
    .map((e) => {
      const who =
        e.kind === "user"
          ? t("conductor.log.you")
          : e.kind === "assistant"
            ? name
            : t("conductor.log.action");
      return `[${time(e.ts)}] ${who}: ${e.text}`;
    })
    .join("\n");
}

/** UI-66: each call's turns and actions, kept on this computer for 7 days. */
export function CallLog() {
  const open = useConductor((s) => s.logOpen);
  const openLog = useConductor((s) => s.openLog);
  const phase = useConductor((s) => s.view.phase);
  const keys = useShortcutKeys();
  const { show: toast } = useToast();
  const [calls, setCalls] = useState<CallLogEntry[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (id?: string) => {
    setError(null);
    try {
      const list = await callLog(id);
      setCalls(list);
      setSelected(id ?? list[0]?.call.id ?? null);
    } catch (e) {
      setError((e as { message?: string }).message ?? t("conductor.log.loadFailed"));
      setCalls([]);
    }
  }, []);

  useEffect(() => {
    if (open) void load();
    else setCalls(null);
  }, [open, load, phase]);

  const entry = calls?.find((c) => c.call.id === selected) ?? null;
  const name = t("conductor.name");
  const subtitle = entry
    ? t("conductor.log.subtitle", {
        date: when(entry.call.startedAt),
        duration: clock((entry.call.connectedSeconds ?? 0) * 1000),
        credits: (entry.call.credits ?? 0).toLocaleString("en-US"),
      })
    : undefined;

  const footer = entry ? (
    <div className="flex gap-2">
      <Button
        variant="secondary"
        onClick={() =>
          void navigator.clipboard
            .writeText(logText(entry, name))
            .then(() => toast({ tone: "success", title: t("conductor.log.copied") }))
            .catch(() => toast({ tone: "error", title: t("conductor.log.copyFailed") }))
        }
      >
        {t("conductor.log.copy")}
      </Button>
      <Button
        variant="danger"
        data-testid="call-log-delete"
        onClick={() =>
          void invoke("conductor_call_delete", { callId: entry.call.id })
            .then(() => load())
            .catch((e: { message?: string }) => setError(e.message ?? t("conductor.log.loadFailed")))
        }
      >
        {t("conductor.log.delete")}
      </Button>
    </div>
  ) : undefined;

  return (
    <Sheet
      open={open}
      onOpenChange={openLog}
      title={t("conductor.callLog")}
      subtitle={subtitle}
      footer={footer}
      closeLabel={t("common.close")}
    >
      <div data-testid="call-log" className="flex flex-col gap-3">
        {error && (
          <Banner tone="error" action={{ label: t("conductor.log.retry"), onClick: () => void load() }}>
            {error}
          </Banner>
        )}
        <SkeletonGate
          loading={calls === null}
          label={t("conductor.log.loading")}
          skeleton={
            <div className="flex flex-col gap-2">
              <Skeleton width="60%" />
              <Skeleton width="80%" />
              <Skeleton width="40%" />
            </div>
          }
        >
          {calls && calls.length === 0 && !error ? (
            <EmptyState
              title={t("conductor.log.empty")}
              body={t("conductor.log.emptyBody", { shortcut: (keys("conductor.call") ?? []).join("") })}
            />
          ) : (
            <>
              {calls && calls.length > 1 && (
                <Select
                  label={t("conductor.log.previous")}
                  value={selected ?? ""}
                  onValueChange={(id) => void load(id)}
                  options={calls.map((c) => ({
                    value: c.call.id,
                    label: t("conductor.log.option", {
                      date: when(c.call.startedAt),
                      duration: clock((c.call.connectedSeconds ?? 0) * 1000),
                    }),
                  }))}
                />
              )}
              <ol className="flex flex-col gap-2" data-testid="call-log-events">
                {entry?.events.map((e, i) => (
                  <li key={i} data-kind={e.kind} className="flex gap-2 text-sm">
                    <span className="w-20 shrink-0 tabular-nums text-xs text-fg-subtle">{time(e.ts)}</span>
                    {e.kind === "action" ? (
                      <span className="flex min-w-0 items-start gap-1 text-fg-muted">
                        {e.ok ? (
                          <Check
                            size={14}
                            aria-label={t("conductor.chip.done")}
                            className="mt-0.5 shrink-0 text-success"
                          />
                        ) : (
                          <X
                            size={14}
                            aria-label={t("conductor.chip.failed")}
                            className="mt-0.5 shrink-0 text-danger"
                          />
                        )}
                        <span className="break-words">{e.text}</span>
                      </span>
                    ) : (
                      <span className="min-w-0 break-words text-md text-fg">
                        {t("conductor.log.turn", {
                          who: e.kind === "user" ? t("conductor.log.you") : name,
                          text: e.text,
                        })}
                      </span>
                    )}
                  </li>
                ))}
              </ol>
            </>
          )}
        </SkeletonGate>
      </div>
    </Sheet>
  );
}
