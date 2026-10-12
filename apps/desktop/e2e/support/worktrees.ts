import { $, browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { home, inputSince, invoke, recordInput, settled } from "./app";

/** Clicks a menu item by its text once the menu is open. */
export async function pick(label: string): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute((l: string) => {
        const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find(
          (el) => el.textContent?.trim() === l,
        );
        item?.click();
        return item !== undefined;
      }, label),
    { timeout: 5_000, timeoutMsg: `no menu item ${label}` },
  );
}

/** Right-clicks the repository's rail row (its context menu). */
export async function repoMenu(root: string): Promise<void> {
  await browser.execute((r: string) => {
    const row = document.querySelector<HTMLElement>(`[data-testid="workspace-row"][data-root="${r}"]`);
    const box = row?.getBoundingClientRect();
    row?.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: (box?.left ?? 0) + 24,
        clientY: (box?.top ?? 0) + 8,
      }),
    );
  }, root);
}

const NAME = '[data-testid="worktree-name"]';
const CREATE = '[data-testid="worktree-create"]';

const laneBranches = async () =>
  (await invoke<Array<{ laneBranch: string | null }>>("workspace_list")).flatMap((w) =>
    w.laneBranch ? [w.laneBranch] : [],
  );

/** Makes a worktree from the repository's menu: New worktree…, the task, Create.
 *
 * Twice in full runs the dialog was still open 20 s after Create (D-839,
 * D-840): once the worktree came only as the wait ran out, once never, with
 * nothing in the app's log meanwhile. Dialogs slide in, and under Xvfb the
 * slide can wait at its first frame until input comes: the pointer goes down
 * on the button where it starts and comes up where the dialog has moved, so
 * the click lands on neither (D-52, D-830). Create waits for the dialog to
 * settle. Create blocks clicks while a create runs, so one more click can
 * only start a create that never started: it's clicked again only when, 5 s
 * on, no click reached Create, Create isn't busy, the field shows no error
 * and no new worktree exists. A click that reached Create and did nothing
 * would be the app's, so that isn't retried. Anything else, and a dialog that
 * closed with no new worktree, fails with what the dialog showed, the input
 * the page received and the tail of the app's log. */
export async function newWorktree(root: string, task: string): Promise<void> {
  await repoMenu(root);
  await pick("New worktree…");
  await $(NAME).setValue(task);
  await $(CREATE).waitForClickable({ timeout: 10_000 });
  await settled();
  const before = await laneBranches();
  const since = await recordInput(CREATE);
  const clicked = Date.now();
  const closed = (timeout: number) =>
    $(NAME)
      .waitForExist({ reverse: true, timeout })
      .then(
        () => true,
        () => false,
      );
  const seen = async () => ({
    ...(await browser.execute(
      (name: string, create: string) => {
        const field = document.querySelector<HTMLInputElement>(name);
        return {
          open: field !== null,
          busy: document.querySelector(create)?.getAttribute("aria-busy") === "true",
          error: field?.getAttribute("aria-invalid") === "true",
          text: document.querySelector<HTMLElement>('[role="dialog"]')?.innerText ?? null,
        };
      },
      NAME,
      CREATE,
    )),
    added: (await laneBranches()).filter((b) => !before.includes(b)),
    input: await inputSince(since),
    ms: Date.now() - clicked,
  });
  const fail = async (what: string) => {
    const log = readFileSync(join(home(), "logs", "tandem.log"), "utf8")
      .trimEnd()
      .split("\n")
      .slice(-12);
    return new Error(
      `the New worktree dialog for "${task}" ${what}: ${JSON.stringify(await seen())}\n${log.join("\n")}`,
    );
  };
  await $(CREATE).click();
  if (!(await closed(5_000))) {
    const first = await seen();
    // A click inside the dialog on Create's box, as the page received it.
    const reached = first.input.some(
      (r) =>
        r.type === "click" &&
        r.inDialog &&
        !!r.watched &&
        r.x >= r.watched[0]! &&
        r.x <= r.watched[0]! + r.watched[2]! &&
        r.y >= r.watched[1]! &&
        r.y <= r.watched[1]! + r.watched[3]!,
    );
    if (first.open && !reached && !first.busy && !first.error && first.added.length === 0) {
      console.warn(
        `newWorktree: the Create click for "${task}" didn't register (${JSON.stringify(first)}); clicking again`,
      );
      await $(CREATE).click();
    }
    if (first.error || !(await closed(20_000))) throw await fail("was still open after Create");
  }
  if ((await laneBranches()).every((b) => before.includes(b)))
    throw await fail("closed with no new worktree");
}
