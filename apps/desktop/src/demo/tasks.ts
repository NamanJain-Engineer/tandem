// The browser demo's task tracker (F-TK-03): tasks kept in the page, with the
// app's rules for what a save, a comment, an archive and a restore change. It
// starts empty, as a new install does, and nothing is saved anywhere. Running
// a task needs your computer, so the demo says it works in the app (D-2905).
import type {
  Issue,
  IssueStatus,
  IssueSummary,
  SaveIssue,
  TaskComment,
  TaskDetail,
  TaskHistoryEntry,
  TaskPage,
  TasksMeta,
} from "@tandem/protocol";

const STATUSES: IssueStatus[] = ["backlog", "todo", "in_progress", "in_review", "done", "canceled"];
const NOT_FOUND = { code: "not_found", message: "That task doesn't exist" };
const CONFLICT = { code: "conflict", message: "Someone else changed this task. Reload it and try again." };
const MAX_TITLE = 200;

type Fields = Pick<Issue, "title" | "description" | "status" | "priority" | "labels" | "project" | "assignee" | "result">;

export class DemoTasks {
  private issues = new Map<string, Issue>();
  private comments = new Map<string, TaskComment[]>();
  private history = new Map<string, TaskHistoryEntry[]>();
  private next = 1;

  constructor(private readonly now: () => number = () => Date.now()) {}

  meta(): TasksMeta {
    return {
      version: 1,
      next_number: this.next,
      projects: [],
      labels: [],
      statuses: STATUSES,
      actors: [{ id: "me", kind: "human", name: "You" }],
      teams: [],
    };
  }

  list(includeArchived: boolean): TaskPage<IssueSummary> {
    const items = [...this.issues.values()]
      .filter((i) => includeArchived || !i.archived)
      .sort((a, b) => b.updated_at - a.updated_at)
      .map((i) => ({
        id: i.id,
        number: i.number,
        revision: i.revision,
        title: i.title,
        status: i.status,
        priority: i.priority,
        labels: i.labels,
        project: i.project,
        assignee: i.assignee,
        updated_at: i.updated_at,
        archived: i.archived,
      }));
    return { items, next_cursor: null };
  }

  get(id: string): TaskDetail {
    const issue = this.issues.get(id);
    if (!issue) throw NOT_FOUND;
    return { issue, comments: this.comments.get(id) ?? [], history: this.history.get(id) ?? [] };
  }

  save(input: SaveIssue): Issue {
    const at = this.now();
    if (input.title !== undefined && !input.title.trim()) throw { code: "invalid", message: "A task needs a title" };
    if (input.title !== undefined && [...input.title].length > MAX_TITLE)
      throw { code: "invalid", message: `A title is at most ${MAX_TITLE} characters` };
    if (!input.id) {
      const number = this.next++;
      const issue: Issue = {
        id: `TAN-${number}`,
        number,
        revision: 1,
        title: (input.title ?? "").trim(),
        description: input.description ?? "",
        status: input.status ?? "todo",
        priority: input.priority ?? 0,
        labels: input.labels ?? [],
        project: input.project ?? null,
        assignee: input.assignee ?? null,
        created_at: at,
        updated_at: at,
        archived: false,
        result: input.result ?? null,
        attempts: [],
        ...(input.suggested_engine ? { suggested_engine: input.suggested_engine } : {}),
      };
      this.issues.set(issue.id, issue);
      return issue;
    }
    const issue = this.issues.get(input.id);
    if (!issue) throw NOT_FOUND;
    if (input.expected_revision !== undefined && input.expected_revision !== issue.revision) throw CONFLICT;
    const patch: Partial<Fields> = {};
    for (const key of ["title", "description", "status", "priority", "labels", "project", "assignee", "result"] as const) {
      if (input[key] !== undefined) Object.assign(patch, { [key]: key === "title" ? input.title?.trim() : input[key] });
    }
    return this.change(issue, patch);
  }

  comment(id: string, body: string, parentId: string | null): TaskComment {
    if (!this.issues.has(id)) throw NOT_FOUND;
    if (!body.trim()) throw { code: "invalid", message: "A comment can't be empty" };
    const list = this.comments.get(id) ?? [];
    const c: TaskComment = { id: `c${list.length + 1}`, issue_id: id, ts: this.now(), actor: "me", body, parent_id: parentId };
    this.comments.set(id, [...list, c]);
    return c;
  }

  setArchived(id: string, archived: boolean, expectedRevision?: number): Issue {
    const issue = this.issues.get(id);
    if (!issue) throw NOT_FOUND;
    if (expectedRevision !== undefined && expectedRevision !== issue.revision) throw CONFLICT;
    return this.change(issue, { archived });
  }

  private change(issue: Issue, patch: Partial<Fields> & { archived?: boolean }): Issue {
    const at = this.now();
    const entries: TaskHistoryEntry[] = [];
    for (const [field, to] of Object.entries(patch)) {
      const from = (issue as Record<string, unknown>)[field];
      if (JSON.stringify(from) !== JSON.stringify(to)) entries.push({ ts: at, actor: "me", connection: null, field, from, to });
    }
    if (!entries.length) return issue;
    const next: Issue = { ...issue, ...patch, revision: issue.revision + 1, updated_at: at };
    this.issues.set(issue.id, next);
    this.history.set(issue.id, [...(this.history.get(issue.id) ?? []), ...entries]);
    return next;
  }
}
