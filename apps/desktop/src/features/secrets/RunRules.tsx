import { useEffect, useState } from "react";
import type { IpcError, RunRuleRepo } from "@tandem/protocol";
import { Button, Callout, Chip, Input, Skeleton } from "@tandem/ui";
import { t } from "../../i18n";
import { runRulesList, runRulesSet } from "../../stores/secrets";

/** One folder's rules: its commands, and a field to add one. */
function RepoRules({ repo, onChange }: { repo: RunRuleRepo; onChange: (r: RunRuleRepo) => void }) {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const save = async (commands: string[]) => {
    setError(null);
    try {
      onChange(await runRulesSet(repo.repo, commands));
      return true;
    } catch (e) {
      setError((e as IpcError).message ?? t("runRules.failed"));
      return false;
    }
  };
  const add = async () => {
    if (draft.trim() === "") return;
    if (await save([...repo.commands, draft])) setDraft("");
  };
  return (
    <li className="flex flex-col gap-1.5" data-testid="run-rules-repo" data-repo={repo.repo}>
      <p className="flex items-baseline gap-2 text-sm">
        <span className="font-semibold text-fg">{repo.name}</span>
        <span className="truncate font-mono text-xs text-fg-subtle" title={repo.repo}>
          {repo.repo}
        </span>
      </p>
      {repo.commands.length > 0 && (
        <div className="flex flex-wrap gap-1.5" data-testid="run-rules-commands">
          {repo.commands.map((c) => (
            <Chip
              key={c}
              label={c}
              className="font-mono"
              removeLabel={(l) => t("runRules.remove", { command: l })}
              onRemove={() => void save(repo.commands.filter((x) => x !== c))}
            />
          ))}
        </div>
      )}
      <form
        className="flex items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <Input
          aria-label={t("runRules.add", { name: repo.name })}
          data-testid="run-rules-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="pnpm deploy"
          monospace
          spellCheck={false}
          error={error ?? undefined}
        />
        <Button
          type="submit"
          size="sm"
          variant="secondary"
          data-testid="run-rules-add"
          disabled={draft.trim() === ""}
        >
          {t("runRules.addButton")}
        </Button>
      </form>
    </li>
  );
}

/** Settings › Secrets › "Commands agents can run without asking" (F-TL-12):
 * per folder, a command and its arguments; `vibezeo run` asks for anything else. */
export function RunRules() {
  const [repos, setRepos] = useState<RunRuleRepo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => {
    setError(null);
    runRulesList()
      .then(setRepos)
      .catch((e: IpcError) => setError(e.message ?? String(e)));
  };
  useEffect(load, []);
  return (
    <section className="flex flex-col gap-2" data-testid="run-rules">
      <h4 className="text-base font-semibold text-fg">{t("runRules.title")}</h4>
      <p className="text-sm text-fg-muted">{t("runRules.body")}</p>
      {error ? (
        <Callout
          tone="error"
          actions={
            <Button size="sm" variant="secondary" onClick={load}>
              {t("secrets.retry")}
            </Button>
          }
        >
          {error}
        </Callout>
      ) : repos === null ? (
        <Skeleton className="h-16" />
      ) : repos.length === 0 ? (
        <p className="text-sm text-fg-muted" data-testid="run-rules-empty">
          {t("runRules.empty")}
        </p>
      ) : (
        <ul className="flex flex-col gap-3">
          {repos.map((r) => (
            <RepoRules
              key={r.repo}
              repo={r}
              onChange={(next) =>
                setRepos((all) => (all ?? []).map((x) => (x.repo === next.repo ? next : x)))
              }
            />
          ))}
        </ul>
      )}
    </section>
  );
}
