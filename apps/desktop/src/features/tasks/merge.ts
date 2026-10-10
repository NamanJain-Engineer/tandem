// F-TK-03 merge rule: unsaved local edits to a field survive another
// writer's update to *other* fields; when that writer changed the same field,
// the field is in conflict until the user keeps theirs or takes the other.
import type { Issue } from "@tandem/protocol";

/** The fields the detail edits locally before saving. */
export type EditField = "title" | "description";

export type Draft = {
  /** The issue as it was when the edits started (or were last rebased). */
  base: Issue;
  /** Unsaved values per field. */
  edits: Partial<Record<EditField, string>>;
  /** Fields another writer changed under an unsaved edit, with their value. */
  conflicts: Partial<Record<EditField, string>>;
};

export function draftOf(issue: Issue): Draft {
  return { base: issue, edits: {}, conflicts: {} };
}

/** Takes a newer copy of the issue: other fields follow it silently; an
 * edited field it also changed becomes a conflict (F-TK-03 AC1, AC2). */
export function rebase(draft: Draft, latest: Issue): Draft {
  if (latest.revision <= draft.base.revision && latest.id === draft.base.id) return draft;
  const conflicts = { ...draft.conflicts };
  for (const field of Object.keys(draft.edits) as EditField[]) {
    if (latest[field] !== draft.base[field] && latest[field] !== draft.edits[field]) {
      conflicts[field] = latest[field];
    }
  }
  // An edit that now equals what's saved needs nothing more.
  const edits = { ...draft.edits };
  for (const field of Object.keys(edits) as EditField[]) {
    if (edits[field] === latest[field]) {
      delete edits[field];
      delete conflicts[field];
    }
  }
  return { base: latest, edits, conflicts };
}

export function edit(draft: Draft, field: EditField, value: string): Draft {
  const edits = { ...draft.edits };
  if (value === draft.base[field] && !(field in draft.conflicts)) delete edits[field];
  else edits[field] = value;
  return { ...draft, edits };
}

/** Drops a local edit (Cancel, or "Take theirs"). */
export function discard(draft: Draft, field: EditField): Draft {
  const edits = { ...draft.edits };
  const conflicts = { ...draft.conflicts };
  delete edits[field];
  delete conflicts[field];
  return { ...draft, edits, conflicts };
}

/** "Keep mine": the edit stays, to be saved over the other change. */
export function keepMine(draft: Draft, field: EditField): Draft {
  const conflicts = { ...draft.conflicts };
  delete conflicts[field];
  return { ...draft, conflicts };
}

/** What the field shows: the unsaved edit, else the saved value. */
export function shown(draft: Draft, field: EditField): string {
  return draft.edits[field] ?? draft.base[field];
}
