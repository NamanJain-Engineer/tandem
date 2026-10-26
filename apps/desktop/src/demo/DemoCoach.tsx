import { useEffect, useState, useSyncExternalStore } from "react";
import { Button, Coachmark } from "@tandem/ui";
import { t } from "../i18n";
import { useThreads } from "../stores/threads";
import { useWorkspaces } from "../stores/workspaces";
import { DEMO_MESSAGE_LIMIT, DEMO_WAITING } from "./backend";

/** The only thing the demo keeps in browser storage: the tips are done. */
export const HINT_KEY = "vibezeo-demo-hint-dismissed";

/** What the coach needs from the demo core: the thread with the ready
 * task, and whether a message went. */
export type CoachCore = {
  firstThread: string;
  subscribe: (fn: () => void) => () => void;
  messagesLeft: () => number;
};

type HintId = "waiting" | "send" | "changes";

/** Where a hint can sit: the first of its spots that's on screen wins. */
type Spot = {
  anchor: string;
  side: "top" | "right" | "bottom";
  align?: "center" | "start";
  text: () => string;
};

/** Three hints, in order, with DESIGN §9's words (§5.2.2). None sits over
 * the rail's content. */
const HINTS: { id: HintId; spots: Spot[] }[] = [
  {
    id: "waiting",
    // To the row's right, beside the rail.
    spots: [
      {
        anchor: `[data-testid="workspace-row"][data-root="${DEMO_WAITING.root}"]`,
        side: "right",
        text: () => t("demo.coach.waiting", { engine: DEMO_WAITING.engine }),
      },
    ],
  },
  {
    id: "send",
    spots: [
      { anchor: '[data-testid="composer"]', side: "top", text: () => t("demo.coach.send") },
      // The demo opens in Code mode (§5.2.1): the task waits one switch away.
      // Under the Thread segment from its left edge, clear of the rail.
      {
        anchor: '[role="radio"][value="thread"]',
        side: "bottom",
        align: "start",
        text: () => t("demo.coach.toThread"),
      },
    ],
  },
  {
    id: "changes",
    spots: [
      { anchor: '[data-testid="changes-toggle"]', side: "bottom", text: () => t("demo.coach.changes") },
    ],
  },
];

type Found = Record<HintId, { el: Element; spot: Spot } | null>;

function stored(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === "1";
  } catch {
    return false;
  }
}

function store(): void {
  try {
    localStorage.setItem(HINT_KEY, "1");
  } catch {
    // Private windows may refuse storage: the tips just come back next time.
  }
}

/** `el` if it's rendered: an inactive mode's rail stays in the page, hidden. */
function shown(el: Element | null): Element | null {
  if (!el || el.closest("[hidden]")) return null;
  if (typeof el.checkVisibility === "function") return el.checkVisibility() ? el : null;
  for (let e: Element | null = el; e; e = e.parentElement) {
    if (e instanceof HTMLElement && e.style.display === "none") return null;
  }
  return el;
}

/** Where each hint's target is on screen, kept current as the app renders. */
function useOnScreen(): Found {
  const find = (): Found =>
    Object.fromEntries(
      HINTS.map((h) => {
        for (const spot of h.spots) {
          const el = shown(document.querySelector(spot.anchor));
          if (el) return [h.id, { el, spot }];
        }
        return [h.id, null];
      }),
    ) as Found;
  const [found, setFound] = useState(find);
  useEffect(() => {
    let frame = 0;
    const check = () =>
      setFound((prev) => {
        const next = find();
        return HINTS.every((h) => prev[h.id]?.el === next[h.id]?.el && prev[h.id]?.spot === next[h.id]?.spot)
          ? prev
          : next;
      });
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(check);
    };
    // Siblings rendered with the coach are in the page only after commit.
    check();
    const mo = new MutationObserver(update);
    // Modes stay mounted and hide with `display: none`, so style changes count.
    mo.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["style", "hidden"],
    });
    return () => {
      cancelAnimationFrame(frame);
      mo.disconnect();
    };
  }, []);
  return found;
}

/** One hint at a time, each beside what it describes and gone once the
 * visitor does it: open the waiting agent, send the ready task, open its
 * Changes. A hint whose target isn't on screen waits its turn; Changes waits
 * until the agent has edited something. */
export function DemoCoach({ core }: { core: CoachCore }) {
  const [hidden, setHidden] = useState(stored);
  const [done, setDone] = useState<ReadonlySet<HintId>>(() => new Set());
  const active = useWorkspaces((s) => s.activeId);
  const sent = useSyncExternalStore(
    (fn) => core.subscribe(fn),
    () => core.messagesLeft() < DEMO_MESSAGE_LIMIT,
  );
  const state = useThreads((s) => s.list.find((x) => x.id === core.firstThread)?.state);
  const changesOpen = useThreads((s) => s.changesOpen[core.firstThread] ?? false);
  const onScreen = useOnScreen();

  // Done stays done, so leaving a view doesn't bring its hint back.
  const now: Record<HintId, boolean> = {
    waiting: active === DEMO_WAITING.workspaceId,
    send: sent,
    changes: changesOpen,
  };
  const ready: Record<HintId, boolean> = { waiting: true, send: true, changes: sent && state !== "working" };
  const fresh = HINTS.filter((h) => now[h.id] && !done.has(h.id))
    .map((h) => h.id)
    .join(",");
  useEffect(() => {
    if (fresh) setDone((d) => new Set([...d, ...(fresh.split(",") as HintId[])]));
  }, [fresh]);
  const all = HINTS.every((h) => done.has(h.id) || now[h.id]);
  useEffect(() => {
    if (all && !hidden) {
      store();
      setHidden(true);
    }
  }, [all, hidden]);

  if (hidden || all) return null;
  const index = HINTS.findIndex((h) => !done.has(h.id) && !now[h.id] && ready[h.id] && onScreen[h.id]);
  const hint = HINTS[index];
  const at = hint ? onScreen[hint.id] : null;
  if (!at) return null;
  const hide = () => {
    store();
    setHidden(true);
  };
  return (
    <Coachmark
      anchor={at.el}
      side={at.spot.side}
      align={at.spot.align}
      label={t("demo.coach.label")}
      footer={
        <>
          <span className="tabular-nums">{t("demo.coach.step", { n: index + 1, total: HINTS.length })}</span>
          <Button size="sm" variant="ghost" onClick={hide}>
            {t("demo.coach.hide")}
          </Button>
        </>
      }
    >
      <p>{at.spot.text()}</p>
    </Coachmark>
  );
}
