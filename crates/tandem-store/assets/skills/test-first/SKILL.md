---
name: test-first
description: Use when fixing a bug or adding behaviour that a test can pin down, by writing the failing test before the fix.
---
# Test first

## When to use
A bug with a reproducible cause, or a new behaviour with a clear expected result.

## Steps
1. Write the smallest test that shows the problem or the missing behaviour. Name it after what should happen.
2. Run it and watch it fail for the reason you expect. A test that fails for another reason proves nothing yet.
3. Make the smallest change that turns it green.
4. Run the neighbouring tests, then the whole suite for the package you changed.
5. Tidy the code with the test still green.

## Check the result
- The new test fails without your change and passes with it.
- No existing test was skipped, deleted or loosened to make the suite pass.

## Don't
- Don't test implementation details the user doesn't care about; test what the code promises.
- Don't leave a flaky test behind. If timing matters, control the clock.
