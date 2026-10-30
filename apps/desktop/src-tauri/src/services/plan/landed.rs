//! Landed is Done (F-TK-09, bold bet B3's fourth slice). When Land
//! worktrees (F-WT-07) lands a plan task's worktree, read from its own
//! result, the task's run and the task are Done, a comment names the
//! commit, and the plan's row says Landed. A worktree landed by hand, or a
//! landing that failed or was stopped, never gets here.

use tandem_tasks::{IssueStatus, Writer};

use super::{PlanLanded, PlanService};

/// The commit as the comment names it.
const SHORT: usize = 12;

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
            });
            e.view.clone()
        };
        self.save();
        self.publish(&view);
        tracing::info!(code = "plan.task_landed", "a plan task's worktree landed");
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
}
