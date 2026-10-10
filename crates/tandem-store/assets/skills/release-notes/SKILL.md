---
name: release-notes
description: Use when drafting release notes or a changelog entry from merged changes, verifying every line against the repository.
---
# Release notes

## When to use
Before a release, or when asked what changed between two versions.

## Steps
1. Find the range: the previous release tag (or the date asked for) up to the current commit.
2. List the merged changes in that range from the history, not from memory.
3. Group them for the reader: New, Improved, Fixed. Leave out internal-only work unless asked.
4. Write each line as what a user can now do or no longer suffers, in plain words, present tense.
5. Check each line against its change. If you can't point to the commit, the line goes.

## Check the result
- Every line maps to a real change in the range.
- Nothing promises a feature, number or date the code doesn't back up.

## Don't
- Don't publish or post the notes anywhere. Hand the draft back for review.
- Don't copy commit messages verbatim when they're written for developers.
