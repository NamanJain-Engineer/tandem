//! Landed is Done (F-TK-09, bold bet B3's fourth slice). When Land
//! worktrees (F-WT-07) lands a plan task's worktree, read from its own
//! result, the task's run and the task are Done, a comment names the
//! commit, and the plan's row says Landed. A worktree landed by hand, or a
//! landing that failed or was stopped, never gets here.

use tandem_tasks::{IssueStatus, Writer};

use super::{PlanLanded, PlanService};

/// The commit as the comment names it.
const SHORT: usize = 12;
/// On a task whose landing was undone.
pub(super) const UNLANDED: &str =
    "Undo landing took this task's merge off the base, so it's in review again.";
/// On one you'd moved on from Done since it landed: its status is yours.
pub(super) const UNLANDED_KEPT: &str = "Undo landing took this task's merge off the base.";

impl PlanService {
    /// Land worktrees landed `lane_id` as `commit`. Nothing for a worktree
    /// that isn't a plan task's, or whose run is still going.
    pub async fn on_landed(&self, lane_id: &str, commit: &str) {
        let Some((issue_id, attempt)) = self.host.lane_run(lane_id).await else {
            return;
        };
        let planned = self
            .lock()
            .values()
            .any(|e| e.view.issue_ids.contains(&issue_id));
        if !planned || !self.host.run_landed(&attempt).await {
            return;
        }
        let now = self.host.now();
        let view = {
            let mut plans = self.lock();
            let Some(e) = plans
                .values_mut()
                .find(|e| e.view.issue_ids.contains(&issue_id))
            else {
                return;
            };
            e.view.landed.retain(|l| l.issue_id != issue_id);
            e.view.landed.push(PlanLanded {
                issue_id: issue_id.clone(),
                commit: commit.to_string(),
                at: now,
                attempt_id: Some(attempt.clone()),
            });
            e.view.clone()
        };
        self.save();
        self.publish(&view);
        tracing::info!(code = "plan.task_landed", "a plan task's worktree landed");
        // A review of it still waiting for you is stale: it showed beside
        // Landed (the review of 0e20d5b7, SF-7).
        self.run_moved_on(&attempt, super::review::LANDED).await;
        let Some(tracker) = self.host.tracker() else {
            return;
        };
        let short: String = commit.chars().take(SHORT).collect();
        let body = format!("Landed by Land worktrees as {short}.");
        let written = tokio::task::spawn_blocking(move || {
            tracker.force_update(&issue_id, Some(IssueStatus::Done), None, &Writer::human())?;
            tracker.add_comment(&issue_id, &body, None, None, &Writer::human())
        })
        .await;
        if !matches!(written, Ok(Ok(_))) {
            tracing::warn!(
                code = "plan.landed_failed",
                "a landed task isn't marked Done"
            );
        }
    }

    /// Undo landing took `commits` (the landed merges) off the base: a plan
    /// task that landed as one of them is in review again, with the run that
    /// landed and a comment, and its row no longer says Landed. It read Done
    /// and Landed with its commit gone from the base (the review of
    /// 0e20d5b7, SF-8). A task you've moved on from Done since keeps your
    /// status (the review of e00668ee, Nit 5).
    pub async fn on_unlanded(&self, commits: &[String]) {
        let (views, issues) = {
            let mut plans = self.lock();
            let mut views = Vec::new();
            let mut issues = Vec::new();
            for e in plans.values_mut() {
                let before = issues.len();
                e.view.landed.retain(|l| {
                    let undone = commits.contains(&l.commit);
                    if undone {
                        issues.push((l.issue_id.clone(), l.attempt_id.clone()));
                    }
                    !undone
                });
                if issues.len() > before {
                    views.push(e.view.clone());
                }
            }
            (views, issues)
        };
        if views.is_empty() {
            return;
        }
        self.save();
        for v in &views {
            self.publish(v);
        }
        tracing::info!(
            code = "plan.task_unlanded",
            tasks = issues.len(),
            "plan tasks' landing was undone"
        );
        for (issue_id, attempt) in issues {
            self.host.run_unlanded(&issue_id, attempt.as_ref()).await;
            let Some(tracker) = self.host.tracker() else {
                continue;
            };
            let written = tokio::task::spawn_blocking(move || {
                let done = tracker.get(&issue_id)?.status == IssueStatus::Done;
                if done {
                    tracker.force_update(
                        &issue_id,
                        Some(IssueStatus::InReview),
                        None,
                        &Writer::human(),
                    )?;
                }
                let body = if done { UNLANDED } else { UNLANDED_KEPT };
                tracker.add_comment(&issue_id, body, None, None, &Writer::human())
            })
            .await;
            if !matches!(written, Ok(Ok(_))) {
                tracing::warn!(
                    code = "plan.unlanded_failed",
                    "an unlanded task isn't back in review"
                );
            }
        }
    }
}
