import { Check, GitBranch } from "lucide-react";
import type { ReactNode } from "react";
import { SAMPLE, failurePrompt, t } from "../../../lib/scenes";
import { Icon } from "../Icon";
import { AppWindow, Changes, DiffRows, Dot, Lines, Pane, StatusBar, Toast, fx, visit, type DiffRow } from "./parts";

/** When things happen, in seconds from the scene's start. */
export const CI_T = {
  /** The rail row's badge is pressed, and the Checks dialog rises. */
  open: 0.8,
  /** "Send failure to agent" is pressed: it types the failure into the
   * agent's terminal and the toast says so, and the agent sets to work. */
  send: 2.0,
  sent: 2.3,
  /** The dialog stays until Close is pressed (SendToAgent doesn't close it). */
  close: 3.0,
  /** The agent's fix: reading, the edit, the tests, the commit and its one
   * line on what it changed; done then. */
  work: 3.9,
  /** Its edit shows in Changes; its commit leaves one to push. */
  edited: 4.5,
  commit: 6.0,
  done: 6.4,
  /** The branch chip is pressed, then Push; pushed. */
  chip: 6.5,
  push: 6.9,
  pushed: 7.4,
  /** Vibezeo reads the checks again: running, then passed. */
  running: 7.8,
  passed: 9.2,
  /** Open diff, then Import review comments from #42; once its toast has
   * gone, Send to the agent. */
  diff: 9.6,
  import: 10.1,
  imported: 10.4,
  ready: 12.0,
} as const;
const T = CI_T;

/** The commented file's hunk in Open diff: the finished worktree's change
 * to it, with its old and new line numbers, as DiffView shows them. It's a
 * run of git's own diff of the demo's sample files (a test checks). */
export const SESSION_DIFF: readonly DiffRow[] = [
  [" ", "const sessions = new Map<string, Session>();", 9, 9],
  [" ", "", 10, 10],
  ["−", "/** The session a request's cookie names, while it's valid. */", 11, null],
  ["−", "export async function getSession(req: Request): Promise<Session | null> {", 12, null],
  ["+", "/** How long after it expires a session can still be renewed. */", null, 11],
  ["+", "export const GRACE_MS = 15 * 60_000;", null, 12],
  ["+", "", null, 13],
  ["+", "/** The session a request's cookie names, renewed when it expired less than", null, 14],
  ["+", " * GRACE_MS ago. Signing in sets a new cookie in one tab while another tab", null, 15],
  ["+", " * still sends the old one, which used to land it back on /login. */", null, 16],
  ["+", "export async function refreshSession(req: Request): Promise<Session | null> {", null, 17],
  [" ", '  const id = readCookie(req, "sid");', 13, 18],
];

/** The pull request's worktree: the finished one, whose agent gets the
 * failure. */
const agent = SAMPLE.worktrees.finished;
const { pr } = SAMPLE;
if (pr.branch !== agent.branch) throw new Error(`scenes: the CI scene needs the pull request on the finished worktree, not ${pr.branch}`);
const failed = pr.checks.filter((c) => c.bucket === "fail");
const [comment] = pr.comments;
if (!comment || comment.file !== pr.session.edit) throw new Error("scenes: the CI scene's diff is of the file the pull request's comment is on");
/** The imported comment under its line: DiffView's annotation row, the
 * reviewer's handle and words, and Remove only (an imported comment isn't
 * edited: the prompt sends GitHub's fenced copy). */
const NOTES = new Map<number, ReactNode>();
const commented = SESSION_DIFF.findIndex(([, , , newNo]) => newNo === comment.line);
if (commented < 0) throw new Error(`scenes: the CI scene's diff has no line ${comment.line} for the pull request's comment`);
NOTES.set(
  commented,
  <span key="note" className="sc-diff-note" {...fx("in", T.imported)}>
    <span className="sc-diff-note-text">
      <span className="sc-diff-note-by">{t("worktree.review.importedBy", { author: comment.author })}</span> {comment.text}
    </span>
    <span className="sc-btn sc-btn-ghost">{t("worktree.review.remove")}</span>
  </span>,
);

/** The agent through the scene: done with its first task, working once the
 * failure is sent, done again after its commit. */
const AGENT: ReadonlyArray<readonly [number, ReactNode]> = [
  [0, <Dot key="a" status="done" />],
  [T.sent, <Dot key="b" status="working" />],
  [T.done, <Dot key="c" status="done" />],
];

/** The rail row's checks (ChecksBadge): failed, then running once the fix is
 * pushed and Vibezeo reads them again, then passed. */
function ChecksBadge() {
  return (
    <Changes
      className="sc-checks"
      steps={[
        [
          0,
          <span key="fail" className="sc-checks-fail" {...fx("press", T.open)}>
            {t("checks.failed", { n: failed.length, names: failed.map((c) => c.name).join(", ") })}
          </span>,
        ],
        [
          T.running,
          <span key="run" className="sc-checks-run">
            <span className="sc-spin" />
            {t("checks.running")}
          </span>,
        ],
        [
          T.passed,
          <span key="pass" className="sc-checks-pass">
            <Icon icon={Check} className="sc-checks-icon" />
            {t("checks.passed")}
          </span>,
        ],
      ]}
    />
  );
}

/** A worktree's row in the rail: its agent's state, its name, and for the
 * pull request's worktree its checks. */
function Row({ name, status, checks = false, className }: { name: string; status: ReactNode; checks?: boolean; className?: string }) {
  return (
    <span className={`sc-lane ${className ?? ""}`}>
      {status}
      <span className="sc-lane-name">{name}</span>
      {checks && <ChecksBadge />}
    </span>
  );
}

/** A check's bucket as ChecksDialog's badge draws it. */
const BUCKET: Record<string, string> = { pass: "sc-badge-done", fail: "sc-badge-error" };

/** "Checks for {branch}" (UI-157), over the window from the badge's press
 * until Close: sending the failure leaves it open. */
function ChecksDialog() {
  return (
    <div className="sc-ci-layer" {...visit(T.open + 0.2, T.close + 0.2 - (T.open + 0.2))}>
      <div className="sc-dialog sc-ci-dialog">
        <span className="sc-dash-title">{t("checks.title", { branch: pr.branch })}</span>
        <span className="sc-toast-body">{t("checks.pr", { n: pr.number })}</span>
        <span className="sc-ci-checks">
          {pr.checks.map((c) => (
            <span key={c.name} className="sc-ci-check">
              <span className="sc-ci-check-head">
                <span>{c.name}</span>
                <span className="sc-ci-check-flow">{pr.workflow}</span>
                <span className={`sc-badge ${BUCKET[c.bucket] ?? "sc-badge-outline"}`}>{t(`checks.bucket.${c.bucket}`)}</span>
              </span>
              {c.bucket === "fail" && (
                <span className="sc-btn sc-btn-sm" {...fx("press", T.send)}>
                  {t("failure.send")}
                </span>
              )}
            </span>
          ))}
        </span>
        <span className="sc-dialog-foot sc-ci-foot">
          <span className="sc-btn">{t("checks.refresh")}</span>
          <span className="sc-btn sc-btn-primary" {...fx("press", T.close)}>
            {t("common.close")}
          </span>
        </span>
      </div>
    </div>
  );
}

/** The Files panel (FilesPanel.tsx): Files | Changes, the branch chip with
 * what there is to push and pull, and its menu; then the Changes view,
 * clean until the agent's edit and clean again once it commits. */
function Files() {
  const clean = (key: string) => (
    <span key={key} className="sc-empty">
      <span className="sc-empty-title">{t("git.clean.title")}</span>
      <span className="sc-empty-body">{t("git.clean.body")}</span>
    </span>
  );
  const counts = (ahead: number) => t("git.branch.short", { ahead, behind: 0 });
  return (
    <div className="sc-ci-files" {...fx("out", T.diff)}>
      <div className="sc-files-head">
        <span className="sc-seg">
          <span className="sc-seg-item">{t("files.view.files")}</span>
          <span className="sc-seg-item sc-seg-on">
            <Changes
              steps={[
                [0, t("files.view.changes")],
                [T.edited, t("files.view.changesCount", { n: 1 })],
                [T.commit, t("files.view.changes")],
              ]}
            />
          </span>
        </span>
        <span className="sc-branch-chip" {...fx("press", T.chip)}>
          <Changes
            steps={[
              [0, <Icon key="a" icon={GitBranch} className="sc-branch-chip-icon" />],
              [T.push + 0.1, <span key="b" className="sc-spin" />],
              [T.pushed, <Icon key="c" icon={GitBranch} className="sc-branch-chip-icon" />],
            ]}
          />
          <span className="sc-branch-chip-name">{pr.branch}</span>
          <Changes
            steps={[
              [0, counts(0)],
              [T.commit, counts(1)],
              [T.pushed, counts(0)],
            ]}
          />
        </span>
        <span className="sc-menu" {...visit(T.chip + 0.15, T.push + 0.25 - (T.chip + 0.15))}>
          <span className="sc-menu-item">{t("git.pull")}</span>
          <span className="sc-menu-item sc-menu-on" {...fx("press", T.push)}>
            {t("git.push")}
          </span>
          <span className="sc-menu-sep" />
          <span className="sc-menu-item">{t("git.switch")}</span>
          <span className="sc-menu-item">{t("git.newBranch")}</span>
        </span>
      </div>
      <div className="sc-files-body">
        <Changes
          className="sc-files-swap"
          steps={[
            [0, clean("a")],
            [
              T.edited,
              <span key="edit" className="sc-files-list">
                <span className="sc-files-section">{t("git.unstaged.title", { n: 1 })}</span>
                <span className="sc-files-row">
                  <span className="sc-ci-letter">M</span>
                  <span className="sc-mono">{pr.fix.file}</span>
                </span>
              </span>,
            ],
            [T.commit, clean("b")],
          ]}
        />
      </div>
    </div>
  );
}

/** Open diff on the worktree (WorktreeReview): "Import review comments from
 * #42" brings the reviewer's comment in under its line, with Remove only
 * (an imported comment isn't edited), and the review bar sends it on. */
function Review() {
  return (
    <div className="sc-ci-review" {...fx("in", T.diff)}>
      <span className="sc-ci-review-top">
        <span className="sc-btn sc-btn-sm" {...fx("press", T.import)}>
          {t("worktree.review.import", { n: pr.number })}
        </span>
      </span>
      <div className="sc-ci-file">
        <span className="sc-ci-file-head">
          <span className="sc-ci-letter">M</span>
          <span className="sc-ci-file-path">{comment.file}</span>
          <span className="sc-add">+{pr.session.add}</span>
          <span className="sc-del">−{pr.session.del}</span>
        </span>
        <DiffRows rows={SESSION_DIFF} notes={NOTES} at={T.imported} />
      </div>
      {/* WorktreeReview's bar, once there are comments: their count, then
          Copy review and Send to the worktree's one agent, by its name. */}
      <span className="sc-review-bar" {...fx("in", T.imported)}>
        <span className="sc-review-count">{t("worktree.review.count", { n: pr.comments.length })}</span>
        <span className="sc-btn sc-btn-sm">{t("worktree.review.copy")}</span>
        <span className="sc-btn sc-btn-sm sc-btn-primary" {...fx("pop", T.ready)}>
          {t("worktree.review.sendTo", { name: agent.name })}
        </span>
      </span>
    </div>
  );
}

/** "CI results and review comments back to the agent" (0.9.0's F-WT-08,
 * D-3001): the finished worktree's pull request fails a check. Send failure
 * to agent types the failure into its agent, marked as data, and Close
 * closes the dialog; the agent fixes and commits, the push sets the checks
 * running, and they pass. Open diff imports the reviewer's comment from the
 * pull request, ready to send on. About 12 s, played once. */
export function ChecksScene() {
  const failure = failed[0];
  const prompt = failurePrompt(pr.number, { name: failure?.name ?? "", workflow: pr.workflow, link: pr.link }, pr.log).split("\n");
  // Typed as one paste: the first line after the prompt mark, the rest under it.
  let first = true;
  const typed = prompt.map((l) => {
    if (l === "") return "";
    const line = `${first ? "› " : "  "}${l}`;
    first = false;
    return line;
  });
  return (
    <AppWindow className="sc-ci">
      <div className="sc-body">
        <aside className="sc-rail">
          <span className="sc-rail-head">{t("rail.workspacesHeader")}</span>
          <span className="sc-ws">
            <span className="sc-ws-name">{SAMPLE.workspace}</span>
          </span>
          {SAMPLE.order.map((w) =>
            w.branch === agent.branch ? (
              <Row key={w.branch} name={w.branch} status={<Changes steps={AGENT} />} checks className="sc-lane-on" />
            ) : (
              <Row key={w.branch} name={w.branch} status={<Dot status={w.branch === SAMPLE.worktrees.working.branch ? "working" : "waiting"} />} />
            ),
          )}
        </aside>
        <div className="sc-ci-main">
          {/* A phone has no rail: the worktree's row keeps its checks here. */}
          <Row name={agent.branch} status={<Changes steps={AGENT} />} checks className="sc-ci-strip" />
          <Pane
            engine={agent.engine}
            name={agent.name}
            title={agent.title}
            branch={agent.branch}
            focused
            doneAt={T.done}
            status={<Changes steps={AGENT} />}
          >
            {/* The end of its first task: it ran the cart's tests only. */}
            {SAMPLE.lines.finished.slice(-2).map((l) => (
              <span key={l} className="sc-line sc-line-old">
                {l}
              </span>
            ))}
            {/* The failure, typed in when it's sent, behind the dialog. On a
                phone the link, the blank lines and the agent's last words
                fold away, so the commit still shows. */}
            <Lines lines={typed} start={T.sent} step={0.08} className="sc-line-wrap" fold={(l) => l === "" || l.startsWith("  Link: ")} />
            <Lines lines={pr.fix.lines} start={T.work} end={T.done} className="sc-line-wrap" fold={(_, i) => i === pr.fix.lines.length - 1} />
          </Pane>
          <div className="sc-ci-side">
            <Files />
            <Review />
          </div>
        </div>
        <Toast title={t("failure.sent", { name: agent.name })} at={T.sent} />
        <Toast title={t("git.pushed", { branch: pr.branch })} at={T.pushed} />
        <Toast title={t("worktree.review.imported", { n: pr.comments.length })} at={T.imported} />
      </div>
      <StatusBar branch={agent.branch} />
      {/* Over the whole window, as the app's overlay is (fixed, inset 0). */}
      <ChecksDialog />
    </AppWindow>
  );
}

export const CI_CAPTION = "A failed check goes to the agent as data. Its fix is pushed and passes, and the reviewer's comment comes in too.";

export function ciSceneText(): string {
  return `${agent.branch}'s pull request #${pr.number} has a failed check, ${failed.map((c) => c.name).join(", ")}, on its rail row. Its Checks dialog lists ${pr.checks.map((c) => c.name).join(", ")}, and Send failure to agent types the check, its link and the end of its log into ${agent.name}, the worktree's agent in ${agent.engine}, marked as untrusted data, not instructions. ${agent.name} finds the cause, fixes ${pr.fix.file} and commits. Push, from the Files panel's branch chip, sends the fix up; Vibezeo reads the checks again, every two minutes while it's in front or on Refresh, and they pass. In Open diff, Import review comments from #${pr.number} brings in ${comment.author}'s comment on ${comment.file} line ${comment.line}, ready to send to ${agent.name}. Vibezeo reads pull requests only for a repository you allow, through your own GitHub CLI.`;
}
