---
name: commit-messages
description: Use when writing a commit message, to follow the repository's convention and explain why the change was made.
---
# Commit messages

## When to use
Every commit you make.

## Steps
1. Look at recent commits and match their format (prefixes, ticket ids, tense).
2. Write a subject of about 50–72 characters that says what the commit does.
3. Leave a blank line, then explain why: the problem, and anything a reviewer wouldn't guess from the diff.
4. Mention follow-up work or known limits in the body.

## Check the result
- Someone reading only the log understands what changed and why.

## Don't
- Don't describe the diff line by line.
- Don't mention files or details unrelated to this commit.
