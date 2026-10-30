// Land worktrees in the browser demo (F-WT-07, UI-155): the core's merge
// queue, simulated on the sample repository's checkouts (merge_queue.rs).
// A worktree is a candidate once its branch has commits main doesn't, it
// can't be ticked with uncommitted tracked changes, and Land refuses while
// main has them. Each landing merges main into the worktree, runs the check
// there and merges it into main as "Merge branch '<branch>'"; Undo takes
// main back to where the run started. Nothing is pushed.
import type { IpcError, LandingCandidate, LandingPlan, LandingRun, LandingStepState } from "@tandem/protocol";

/** What the queue needs from the demo's world. */
export type LandingWorld = {
  repoId: string;
  repoName: string;
  base: string;
  /** main's commit now (its newest). */
  head: () => string;
  /** Tracked files with uncommitted changes in a checkout. */
  dirty: (id: string) => number;
  /** The worktrees with commits main doesn't have, oldest first. */
  lanes: () => Array<{ id: string; branch: string; ahead: number }>;
  /** What the check prints in a worktree once main is merged into it, or why
   * the demo can't run that command. */
  check: (id: string, command: string) => { output: string } | { refused: string };
  /** Merges main into the worktree, then the worktree into main: the merge
   * commit it lands as. */
  land: (id: string) => string;
  /** Takes main back to the commit `before`, dropping the landed merges. */
  undo: (before: string) => void;
};

/** How long each step of a landing takes in the demo, in ms. */
const STEP_MS: Record<"merging_base" | "checking" | "landing", number> = {
  merging_base: 700,
  checking: 1_400,
  landing: 500,
};

const fail = (code: IpcError["code"], message: string): IpcError => ({
  code,
  message,
  stage: "landing",
  hint: null,
  retryable: false,
});

export class DemoLanding {
  private run: LandingRun | null = null;
  /** The check last used here: `pnpm test`, from a landing since undone,
   * which leaves the check saved and nothing else. */
  private lastCheck: string | null;
  private undoRecord: { base: string; before: string; landed: number } | null = null;
  private stopping = false;

  constructor(
    private readonly world: LandingWorld,
    private readonly later: (ms: number, fn: () => void) => void,
    lastCheck: string | null,
  ) {
    this.lastCheck = lastCheck;
  }

  plan(): LandingPlan {
    const candidates: LandingCandidate[] = this.world.lanes().map((l) => ({
      laneId: l.id,
      branch: l.branch,
      ahead: l.ahead,
      // Only finished work lands, and no two finished worktrees change the
      // same lines, so every forecast is clean (and the order is the oldest first).
      forecast: { kind: "clean" },
      uncommitted: this.world.dirty(l.id),
      missing: false,
    }));
    return {
      repoId: this.world.repoId,
      base: this.world.base,
      baseDirty: this.world.dirty(this.world.repoId) > 0,
      check: this.lastCheck,
      candidates,
      undo:
        this.undoRecord && this.undoRecord.landed > 0
          ? { base: this.undoRecord.base, landed: this.undoRecord.landed }
          : null,
      interrupted: null,
    };
  }

  get(): LandingRun | null {
    return this.run;
  }

  start(laneIds: string[], check: string | null): LandingRun {
    if (this.run?.state === "running") throw fail("conflict", "A landing is already running here");
    if (laneIds.length === 0) throw fail("invalid", "Choose a worktree to land");
    if (this.world.dirty(this.world.repoId) > 0) {
      throw fail("conflict", `${this.world.repoName} has uncommitted changes. Commit or stash them, then land.`);
    }
    const lanes = this.world.lanes();
    const chosen = laneIds.map((id) => {
      const lane = lanes.find((l) => l.id === id);
      if (!lane) throw fail("invalid", "That worktree isn't in this repository");
      if (this.world.dirty(id) > 0) {
        throw fail("conflict", `${lane.branch} has uncommitted changes. Commit or stash them, then land.`);
      }
      return lane;
    });
    const command = check?.trim() || null;
    if (command) {
      // Refused before anything lands, as the core refuses what it can up front.
      const probe = this.world.check(chosen[0]?.id ?? "", command);
      if ("refused" in probe) throw fail("unavailable", probe.refused);
      this.lastCheck = command;
    }
    const before = this.world.head();
    this.run = {
      repoId: this.world.repoId,
      base: this.world.base,
      before,
      check: command,
      state: "running",
      steps: chosen.map((l) => ({
        laneId: l.id,
        branch: l.branch,
        state: "waiting",
        commit: null,
        exit: null,
        output: null,
        conflicts: [],
        prompt: null,
      })),
      error: null,
    };
    this.stopping = false;
    this.next(0, before, 0);
    return this.run;
  }

  /** The queue stops before its next step; what landed stays landed. */
  stop(): void {
    if (this.run?.state === "running") this.stopping = true;
  }

  undo(): void {
    if (this.run?.state === "running") throw fail("conflict", "Wait for the landing to finish, or stop it");
    const record = this.undoRecord;
    if (!record || record.landed === 0) throw fail("not_found", "There's no landing to undo");
    if (this.world.dirty(this.world.repoId) > 0) {
      throw fail("conflict", `${this.world.repoName} has uncommitted changes. Commit or stash them, then undo.`);
    }
    this.world.undo(record.before);
    this.undoRecord = null;
    this.run = null;
  }

  /** Lands step `i` and on, one at a time. */
  private next(i: number, before: string, landed: number): void {
    const run = this.run;
    if (!run || run.state !== "running") return;
    const step = run.steps[i];
    if (!step) {
      this.set({ state: "done" });
      return;
    }
    // Stopped between worktrees: the next one stays waiting.
    if (this.stopping) return this.set({ state: "stopped" });
    this.stepTo(i, "merging_base");
    this.later(STEP_MS.merging_base, () => {
      if (this.stopping) return this.halt(i, "stopped");
      const land = () => {
        this.stepTo(i, "landing");
        this.later(STEP_MS.landing, () => {
          const commit = this.world.land(step.laneId);
          this.stepTo(i, "landed", { commit });
          // A run that lands something replaces the undo record.
          this.undoRecord = { base: run.base, before, landed: landed + 1 };
          this.next(i + 1, before, landed + 1);
        });
      };
      if (!run.check) return land();
      this.stepTo(i, "checking");
      this.later(STEP_MS.checking, () => {
        if (this.stopping) return this.halt(i, "stopped");
        const out = this.world.check(step.laneId, run.check ?? "");
        if ("refused" in out) return this.halt(i, "stopped", out.refused);
        land();
      });
    });
  }

  private stepTo(i: number, state: LandingStepState, extra: { commit?: string } = {}): void {
    const run = this.run;
    if (!run) return;
    this.run = {
      ...run,
      steps: run.steps.map((s, k) => (k === i ? { ...s, state, ...extra } : s)),
    };
  }

  private halt(i: number, state: LandingStepState, error?: string): void {
    this.stepTo(i, state);
    this.set({ state: "stopped", ...(error ? { error } : {}) });
  }

  private set(next: Partial<LandingRun>): void {
    if (this.run) this.run = { ...this.run, ...next };
  }
}
