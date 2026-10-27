import type { FileDiff, IpcError, RepoReview, ReviewItem, ReviewQueueView, ReviewRisk } from "@tandem/protocol";
import { emitLocal } from "../ipc/events";

const fail = (code: IpcError["code"], message: string): IpcError => ({
  code,
  message,
  stage: null,
  hint: null,
  retryable: false,
});

/** The core's auth rule (review_queue.rs `is_auth`): a path with one of
 * these words in a folder or file name. The sample's other paths fire no
 * other rule (no migrations, CI, lockfiles, env files or deleted tests). */
const AUTH_WORDS = ["auth", "oauth", "login", "password", "secret", "crypto", "permission"];

function risksOf(files: FileDiff[]): ReviewRisk[] {
  const auth = files
    .map((f) => f.path)
    .filter((p) =>
      p.split("/").some((seg) => {
        const s = seg.toLowerCase();
        return !s.startsWith("author") && AUTH_WORDS.some((w) => s.includes(w));
      }),
    );
  return auth.length ? [{ rule: "auth", files: auth.slice(0, 5) }] : [];
}

/** A finished agent's work waiting for review: where, by whom, its last
 * words, its diff, and what the repository's check printed on it. */
export type ReviewSeed = {
  ws: { id: string; name: string };
  /** The repository the worktree belongs to (its check and API paths). */
  repo: { root: string; check: string };
  branch: string;
  paneId: string;
  engine: { id: string; label: string };
  closing: string;
  files: FileDiff[];
  checkOutput: string;
};

/** The review queue in the browser demo (F-DB-04): the finished agent's
 * work in its worktree, with the repository's check passing. Approve,
 * Snooze and Run check work in the page, and the queue updates as in the
 * app. */
export class DemoReview {
  private item: ReviewItem;
  private repo: RepoReview;
  private readonly files: FileDiff[];
  private readonly output: string;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(seed: ReviewSeed, now = Date.now()) {
    this.files = seed.files;
    this.output = seed.checkOutput;
    this.item = {
      key: `pane:${seed.paneId}`,
      source: { type: "pane", paneId: seed.paneId },
      title: seed.engine.label,
      engineId: seed.engine.id,
      workspaceId: seed.ws.id,
      workspace: seed.ws.name,
      branch: seed.branch,
      laneId: seed.ws.id,
      finishedAt: now - 12 * 60_000,
      closing: seed.closing,
      files: seed.files.length,
      additions: seed.files.reduce((n, f) => n + f.additions, 0),
      deletions: seed.files.reduce((n, f) => n + f.deletions, 0),
      risks: risksOf(seed.files),
      check: { state: "passed", exit: 0, output: seed.checkOutput },
      diffHash: `demo-${seed.ws.name}`,
      state: "review",
      snoozedUntil: null,
      turnCheckpointId: null,
    };
    this.repo = { root: seed.repo.root, check: seed.repo.check, apiPaths: [] };
  }

  view(): ReviewQueueView {
    return { items: [this.item], overlaps: [] };
  }

  diff(key: string): FileDiff[] {
    this.must(key);
    return this.files;
  }

  approve(key: string, diffHash: string): ReviewItem {
    this.must(key);
    if (diffHash !== this.item.diffHash) throw fail("conflict", "It changed since you read it.");
    return this.set({ state: "approved" });
  }

  unapprove(key: string): ReviewItem {
    this.must(key);
    return this.set({ state: "review" });
  }

  /** `minutes` 0 brings it back; otherwise an hour, as in the app. */
  snooze(key: string, minutes: number | null): ReviewItem {
    this.must(key);
    if (minutes === 0) return this.set({ state: "review", snoozedUntil: null });
    return this.set({ state: "snoozed", snoozedUntil: Date.now() + (minutes ?? 60) * 60_000 });
  }

  /** The check runs again: running, then passed a moment later. */
  runCheck(key: string): ReviewItem {
    this.must(key);
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.set({ check: { state: "passed", exit: 0, output: this.output } });
    }, 600);
    return this.set({ check: { state: "running", exit: null, output: "" } });
  }

  repoGet(): RepoReview {
    return this.repo;
  }

  repoSet(check: string | null, apiPaths: string[]): RepoReview {
    this.repo = { ...this.repo, check: check?.trim() || null, apiPaths };
    return this.repo;
  }

  dispose(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  private set(next: Partial<ReviewItem>): ReviewItem {
    this.item = { ...this.item, ...next };
    emitLocal("review:queue", this.view());
    return this.item;
  }

  private must(key: string): void {
    if (key !== this.item.key) throw fail("not_found", "That isn't in the review queue.");
  }
}
