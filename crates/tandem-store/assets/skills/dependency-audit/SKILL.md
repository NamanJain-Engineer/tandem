---
name: dependency-audit
description: Use when asked to check a project's dependencies for known vulnerabilities, outdated versions or unused packages.
---
# Dependency audit

## When to use
A periodic check, or before a release.

## Steps
1. Identify the package managers in the project from their lockfiles.
2. Run each ecosystem's own audit and outdated commands, read-only.
3. For each finding, note the package, the installed and fixed versions, and whether it's direct or transitive.
4. Rank: known vulnerabilities reachable from the code first, then major versions behind, then minor updates.
5. Look for packages the code no longer imports.
6. Propose upgrades in small groups that can be tested on their own.

## Check the result
- Each recommendation says what to change and how to verify it (which tests or commands).

## Don't
- Don't upgrade major versions without asking.
- Don't change lockfiles by hand; use the package manager.
