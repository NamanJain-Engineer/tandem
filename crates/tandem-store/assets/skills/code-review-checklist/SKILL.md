---
name: code-review-checklist
description: Use when reviewing a change someone else wrote, to check correctness, tests, safety and clarity in a fixed order.
---
# Code review checklist

## When to use
Reviewing a pull request, a patch or a diff before it lands.

## Steps
1. Read the description and decide what the change is supposed to do.
2. Correctness: trace the main path and each edge case (empty input, errors, concurrency, large inputs).
3. Tests: is the new behaviour covered? Would the tests fail if the change were reverted?
4. Safety: secrets in code or logs, unchecked input reaching a shell, a query or a file path, permissions widened.
5. Clarity: names, comments that explain why, dead code.
6. Write findings most severe first, each with the file, the line and a concrete failure.

## Check the result
- Every finding names a realistic way it goes wrong, or is marked as a suggestion.

## Don't
- Don't rewrite the change in your own style. Point out what matters.
- Don't approve something you couldn't follow; say what was unclear.
