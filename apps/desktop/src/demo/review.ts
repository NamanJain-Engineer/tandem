import type { FileDiff, IpcError, RepoReview, ReviewItem, ReviewQueueView } from "@tandem/protocol";
import { emitLocal } from "../ipc/events";
import { DEMO_CHANGES, TEST_OUTPUT } from "./agent";

const fail = (code: IpcError["code"], message: string): IpcError => ({
  code,
  message,
  stage: null,
  hint: null,
  retryable: false,
});

/** The review queue in the browser demo (F-DB-04): Claude Code's finished
 * fix in `shop`, with the repository's check passing. Approve, Snooze and
 * Run check work in the page, and the queue updates as in the app. */
export class DemoReview {
  private item: ReviewItem;
  private repo: RepoReview;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(ws: { id: string; name: string; root: string }, paneId: string, now = Date.now()) {
    this.item = {
      key: `pane:${paneId}`,
      source: { type: "pane", paneId },
      title: "Claude Code",
      engineId: "claude",
      workspaceId: ws.id,
      workspace: ws.name,
      branch: "main",
      laneId: null,
      finishedAt: now - 12 * 60_000,
      closing: "Fixed, with a test. 13 tests pass.",
      files: DEMO_CHANGES.length,
      additions: DEMO_CHANGES.reduce((n, f) => n + f.additions, 0),
      deletions: DEMO_CHANGES.reduce((n, f) => n + f.deletions, 0),
      risks: [],
      check: { state: "passed", exit: 0, output: TEST_OUTPUT },
      diffHash: "demo-redirect-fix",
      state: "review",
      snoozedUntil: null,
      turnCheckpointId: null,
    };
    this.repo = { root: ws.root, check: "npm test", apiPaths: [] };
  }

  view(): ReviewQueueView {
    return { items: [this.item] };
  }

  diff(key: string): FileDiff[] {
    this.must(key);
    return DEMO_CHANGES;
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
      this.set({ check: { state: "passed", exit: 0, output: TEST_OUTPUT } });
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
