// The browser demo's Plan with Lead (F-TK-06). The demo runs no engines and
// has no teammates, so Lead's plan is simulated: a fixed draft for the goal,
// said to be simulated. Create tasks writes into the demo's tracker; running
// tasks is Pro, which the download (and so the demo) doesn't have.
import type { PlanEngine, PlanTask, PlanView } from "@tandem/protocol";
import { emitLocal } from "../ipc/events";
import type { DemoTasks } from "./tasks";

const ENGINES: PlanEngine[] = [
  { engineId: "claude", label: "Claude Code", accounts: [] },
  { engineId: "codex", label: "Codex", accounts: [] },
];

const NOT_FOUND = { code: "not_found", message: "That plan is gone" };

/** What a planning engine might answer for any goal: split, build, check. */
function draft(goal: string): PlanTask[] {
  const what = goal.split("\n")[0]?.trim() || "the goal";
  return [
    {
      title: `Map what ${what} touches`,
      description:
        "List the files and modules to change, with a short note on each. Done when the list is in a comment.",
      engineId: "claude",
      why: "Simulated in the demo: Lead reads your repository in the app.",
      fellBack: false,
    },
    {
      title: `Build ${what}`,
      description: "Make the change with tests. Done when the tests pass.",
      engineId: "codex",
      why: "Simulated in the demo.",
      fellBack: false,
    },
    {
      title: "Update the docs",
      description: "Describe the change where users will look for it.",
      engineId: "claude",
      why: "Simulated in the demo.",
      fellBack: false,
    },
  ];
}

export class DemoPlans {
  private plans = new Map<string, PlanView>();
  private n = 0;

  constructor(
    private readonly tasks: DemoTasks,
    private readonly later: (ms: number, fn: () => void) => void,
    private readonly now: () => number = () => Date.now(),
  ) {}

  private put(v: PlanView): PlanView {
    this.plans.set(v.id, v);
    emitLocal("tasks:plan", v);
    return v;
  }

  private must(id: string): PlanView {
    const v = this.plans.get(id);
    if (!v) throw NOT_FOUND;
    return v;
  }

  list(): PlanView[] {
    return [...this.plans.values()].sort((a, b) => b.createdAt - a.createdAt);
  }

  start(workspaceId: string, goal: string): PlanView {
    if (!goal.trim()) throw { code: "invalid", message: "Say what the goal is" };
    this.n += 1;
    const v: PlanView = {
      id: `pl_demo${this.n}`,
      goal: goal.trim(),
      workspaceId,
      // No thread: the demo has no Lead to open.
      threadId: "",
      state: "planning",
      tasks: [],
      engines: ENGINES,
      leadId: "",
      leadEngine: "claude",
      error: null,
      projectId: null,
      issueIds: [],
      createdAt: this.now(),
    };
    this.plans.set(v.id, v);
    this.later(1_500, () => {
      const cur = this.plans.get(v.id);
      if (cur?.state === "planning") this.put({ ...cur, state: "ready", tasks: draft(cur.goal) });
    });
    return v;
  }

  createTasks(id: string, tasks: PlanTask[]): PlanView {
    const v = this.must(id);
    if (v.state !== "ready") throw { code: "invalid", message: "Only a finished draft can become tasks" };
    if (!tasks.length || tasks.some((t) => !t.title.trim()))
      throw { code: "invalid", message: "Every task needs a title" };
    const ids = tasks.map(
      (t) =>
        this.tasks.save({
          title: t.title,
          description: t.description,
          status: "todo",
          suggested_engine: t.engineId,
        }).id,
    );
    emitLocal("tasks:changed", { ids });
    return this.put({ ...v, tasks, issueIds: ids, state: "created" });
  }

  execute(): never {
    throw {
      code: "forbidden_plan",
      message: "Running tasks is part of Pro. You can still track tasks for free.",
    };
  }

  discard(id: string): null {
    this.plans.delete(id);
    return null;
  }
}
