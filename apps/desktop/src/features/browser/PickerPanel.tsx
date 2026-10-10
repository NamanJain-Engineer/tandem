// The element picker's panel (F-CD-18), under the browser's toolbar: what's
// picked in the page, a note, and the brief to copy or send to an agent pane
// or a thread's composer. Sending pastes it; nothing is submitted.
import { useState } from "react";
import { MousePointerClick } from "lucide-react";
import type { PickedElement } from "@tandem/protocol";
import { Button, Menu, Textarea, useToast, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { useBrowser } from "../../stores/browser";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { useThreads } from "../../stores/threads";
import { terminalHandles } from "../code/TerminalView";
import { insertIntoComposer } from "../thread/drops";

const NONE: PickedElement[] = [];

/** Where a brief can go: agent panes, and the open thread's composer. */
function useTargets(): Array<{ id: string; label: string; send: (text: string) => boolean }> {
  const tabs = useLayout((s) => s.tabs);
  const label = useEngines((s) => s.label);
  const thread = useThreads((s) => s.list.find((x) => x.id === s.selectedId) ?? null);
  const panes = Object.values(tabs)
    .flatMap((tab) => Object.values(tab.panes))
    .filter((p) => p.kind === "agent")
    .sort((a, b) => a.createdAt - b.createdAt);
  return [
    ...panes.map((p) => {
      const engine = label(p.engineId) ?? p.engineId ?? "";
      return {
        id: p.id,
        label: p.humanName ? t("browser.pick.toPane", { name: p.humanName, engine }) : engine,
        send: (text: string) => {
          const handle = terminalHandles.get(p.id);
          handle?.paste(text);
          return handle !== undefined;
        },
      };
    }),
    ...(thread
      ? [
          {
            id: `thread:${thread.id}`,
            label: t("browser.pick.toThread", { title: thread.title }),
            send: (text: string) => {
              insertIntoComposer(thread.id, text);
              return true;
            },
          },
        ]
      : []),
  ];
}

export function PickerPanel({ id, picking }: { id: string; picking: boolean }) {
  const picked = useBrowser((s) => s.picked[id] ?? NONE);
  const [note, setNote] = useState("");
  const { show } = useToast();
  const targets = useTargets();

  const brief = async (): Promise<string | null> => {
    const md = await useBrowser.getState().brief(id, note);
    if (typeof md === "string") return md;
    show({ tone: "error", title: t("browser.pick.failed"), body: md.message });
    return null;
  };
  const copy = async () => {
    const md = await brief();
    if (md !== null && (await copyText(md))) show({ title: t("browser.pick.copied") });
  };
  const sendTo = async (target: (typeof targets)[number]) => {
    const md = await brief();
    if (md === null) return;
    if (target.send(md)) show({ title: t("browser.pick.sent", { to: target.label }) });
    else show({ tone: "error", title: t("browser.pick.sendFailed", { to: target.label }) });
  };
  const entries: MenuEntry[] =
    targets.length > 0
      ? targets.map((target) => ({ id: target.id, label: target.label, onSelect: () => void sendTo(target) }))
      : [{ id: "none", label: t("browser.pick.noTargets"), disabled: true }];

  return (
    <section aria-label={t("browser.pick.panel")} data-testid="browser-picker" className="flex max-h-[45%] shrink-0 flex-col gap-2 overflow-auto border-b border-divider bg-surface-2 p-2">
      {picking && (
        <p className="flex items-center gap-1.5 text-sm text-fg-muted">
          <MousePointerClick size={14} aria-hidden="true" className="shrink-0" />
          {t("browser.pick.hint")}
        </p>
      )}
      {picked.length === 0 ? (
        <p className="text-sm text-fg-subtle">{t("browser.pick.empty")}</p>
      ) : (
        <ol className="flex flex-col gap-1" aria-label={t("browser.pick.count", { n: picked.length })}>
          {picked.map((e, i) => (
            <li key={`${i}:${e.selector}`} data-testid="browser-picked" className="flex min-w-0 items-baseline gap-2 text-sm">
              <span className="w-4 shrink-0 text-right tabular-nums text-fg-subtle">{i + 1}</span>
              <code className="min-w-0 truncate font-mono text-xs text-fg">{e.selector}</code>
              {e.component && (
                <span className="shrink-0 text-xs text-fg-muted">
                  {t("browser.pick.component", { name: e.component.name, framework: e.component.framework })}
                </span>
              )}
            </li>
          ))}
        </ol>
      )}
      <Textarea
        label={t("browser.pick.note")}
        placeholder={t("browser.pick.notePlaceholder")}
        minRows={2}
        maxRows={5}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        data-testid="browser-pick-note"
      />
      <div className="flex items-center gap-2">
        <Button size="sm" variant="secondary" disabled={picked.length === 0} onClick={() => void copy()} data-testid="browser-brief-copy">
          {t("browser.pick.copy")}
        </Button>
        <Menu
          trigger={
            <Button size="sm" variant="secondary" disabled={picked.length === 0} data-testid="browser-brief-send">
              {t("browser.pick.send")}
            </Button>
          }
          entries={entries}
        />
        <span className="flex-1" />
        {picking ? (
          <Button size="sm" variant="ghost" onClick={() => void useBrowser.getState().pick(id, false)}>
            {t("browser.pick.stop")}
          </Button>
        ) : (
          // Done with this selection: the panel goes (a new pick starts afresh).
          <Button size="sm" variant="ghost" onClick={() => useBrowser.setState((s) => ({ picked: { ...s.picked, [id]: [] } }))}>
            {t("browser.pick.close")}
          </Button>
        )}
      </div>
    </section>
  );
}
