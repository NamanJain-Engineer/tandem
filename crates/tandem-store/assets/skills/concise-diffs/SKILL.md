---
name: concise-diffs
description: Use when changing existing code, to keep each change as small and reviewable as the task allows.
---
# Concise diffs

## When to use
Any edit to code that already exists: a bug fix, a small feature, a refactor someone asked for.

## Steps
1. Restate the change in one sentence before editing. If it needs "and", consider two changes.
2. Find the narrowest place the change can live. Prefer editing one function over touching its callers.
3. Match the surrounding style: naming, comment density, error handling. Don't reformat lines you didn't change.
4. Leave unrelated problems alone. Note them at the end of your reply instead of fixing them in passing.
5. Remove anything you added only to explore (debug prints, commented-out code, scratch files).

## Check the result
- Read the full diff once. Every changed line should be explainable by the one-sentence goal.
- Run the tests that cover the files you touched.

## Don't
- Don't rename, move or reorder code unless the task is about that.
- Don't add dependencies for something a few lines can do.
