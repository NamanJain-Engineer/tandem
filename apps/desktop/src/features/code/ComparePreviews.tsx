import { useEffect, useState } from "react";
import { Banner, Button, Checkbox, Dialog, Skeleton } from "@tandem/ui";
import { t } from "../../i18n";
import { useWorkspaces } from "../../stores/workspaces";
import { comparePreviews, pollPreviews, useWorktrees } from "../../stores/worktrees";
import { switchMode } from "../shell/actions";

/** UI-154 "Compare previews": pick two worktrees; their previews open side
 * by side in a new tab of the repository (F-WT-09). */
export function ComparePreviewsDialog({
  candidates,
  chosen: initial,
  onDone,
  onClose,
}: {
  candidates: string[];
  chosen: string[];
  onDone?: () => void;
  onClose: () => void;
}) {
  const list = useWorkspaces((s) => s.list);
  const previews = useWorktrees((s) => s.previews);
  const [chosen, setChosen] = useState<string[]>(initial.slice(0, 2));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const key = candidates.join(",");
  useEffect(() => {
    void pollPreviews(key ? key.split(",") : []);
  }, [key]);

  const rows = candidates
    .map((id) => list.find((w) => w.id === id))
    .filter((w): w is NonNullable<typeof w> => w !== undefined);
  const loading = rows.some((w) => previews[w.id] === undefined);
  const usable = rows.filter((w) => previews[w.id] && previews[w.id]?.status !== "none");
  const toggle = (id: string, on: boolean) =>
    setChosen((c) => (on ? [...c.filter((x) => x !== id), id].slice(-2) : c.filter((x) => x !== id)));

  const open = async () => {
    const [a, b] = chosen;
    if (!a || !b) return;
    setBusy(true);
    setError(null);
    const out = await comparePreviews([a, b]);
    setBusy(false);
    if (out.ok) {
      switchMode("code");
      onDone?.();
      onClose();
    } else if (out.error.code === "permission_denied") {
      // The review for the untrusted preview took this dialog's place.
    } else setError(out.error.message);
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.compare.title")}
      description={t("worktree.compare.description")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={chosen.length !== 2}
            onClick={() => void open()}
            data-testid="compare-previews-open"
          >
            {busy ? t("worktree.compare.waiting") : t("worktree.compare.open")}
          </Button>
        </>
      }
    >
      <div data-testid="compare-previews" className="flex flex-col gap-2">
        {loading ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton height={32} />
            <Skeleton height={32} />
          </div>
        ) : usable.length < 2 ? (
          <p data-testid="compare-previews-empty" className="text-sm text-fg-subtle">
            {t("worktree.compare.empty")}
          </p>
        ) : (
          usable.map((w) => {
            const status = previews[w.id]?.status;
            return (
              <div
                key={w.id}
                data-testid="compare-previews-row"
                data-lane={w.id}
                data-branch={w.laneBranch ?? ""}
              >
                <Checkbox
                  label={w.laneBranch ?? w.name}
                  description={
                    status === "ready" ? t("worktree.compare.running") : t("worktree.compare.starts")
                  }
                  checked={chosen.includes(w.id)}
                  disabled={busy}
                  onCheckedChange={(on) => toggle(w.id, on)}
                />
              </div>
            );
          })
        )}
        {error ? <Banner tone="error">{error}</Banner> : null}
      </div>
    </Dialog>
  );
}
