import { useState } from "react";
import { Button, Callout } from "@tandem/ui";
import { t } from "../i18n";

/** The only thing the demo keeps in browser storage. */
export const HINT_KEY = "vibezeo-demo-hint-dismissed";

function dismissed(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === "1";
  } catch {
    return false;
  }
}

const STEPS = ["demo.try.send", "demo.try.approve", "demo.try.changes", "demo.try.code", "demo.try.shell", "demo.try.palette"] as const;

/** First steps: how to send the ready message, and what to try next. */
export function DemoHints() {
  const [hidden, setHidden] = useState(dismissed);
  if (hidden) return null;
  const dismiss = () => {
    setHidden(true);
    try {
      localStorage.setItem(HINT_KEY, "1");
    } catch {
      // Private windows may refuse storage: the hint just comes back next time.
    }
  };
  return (
    <div data-testid="demo-hints" className="pointer-events-auto absolute bottom-10 left-1 z-[var(--z-popover)] w-[236px] max-w-[calc(100%-0.5rem)]">
      <Callout
        tone="accent"
        title={t("demo.hint.title")}
        actions={
          <Button size="sm" variant="primary" onClick={dismiss}>
            {t("demo.hint.dismiss")}
          </Button>
        }
      >
        <p>{t("demo.hint.body")}</p>
        <p className="mt-2 font-medium text-fg">{t("demo.try.title")}</p>
        <ol className="mt-1 list-decimal pl-4">
          {STEPS.map((k) => (
            <li key={k}>{t(k)}</li>
          ))}
        </ol>
      </Callout>
    </div>
  );
}
