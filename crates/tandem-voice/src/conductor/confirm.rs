//! Questions Conductor asks before an action (F-CN-03, UI-61): each has its
//! own request id, and an answer resolves that id and no other.
//!
//! - Voice-or-screen questions (closing a pane, executing a task, creating
//!   a teammate) take "yes" by voice, naming the request, or a click.
//! - Screen-only questions (a shell command) take only a click in the main
//!   window, proved by a [`LocalReview`]; a voice "yes" is refused
//!   (F-TL-06 rule 5).
//!
//! The HUD shows one question at a time, the oldest; the rest wait.

use std::sync::Arc;

use parking_lot::Mutex;
use tandem_core::origin::LocalReview;
use tandem_core::voice_tools::Review;
use tokio::sync::oneshot;

use super::view::QuestionView;

/// How a question was answered.
#[derive(Debug)]
pub enum Answer {
    No,
    /// Yes by voice: never for a screen-only question.
    YesByVoice,
    /// Yes by a click in the main window.
    YesOnScreen(LocalReview),
    /// The call ended first.
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfirmError {
    #[error("There's no such question, or it was already answered")]
    NotFound,
    #[error("That needs a click on screen")]
    NeedsScreen,
}

struct Pending {
    view: QuestionView,
    tx: oneshot::Sender<Answer>,
}

type OnChange = Arc<dyn Fn(Option<QuestionView>) + Send + Sync>;

/// Every open question of a call.
pub struct Confirms {
    pending: Mutex<Vec<Pending>>,
    on_change: OnChange,
}

impl Confirms {
    /// `on_change` hears the question the HUD should show now.
    pub fn new(on_change: OnChange) -> Self {
        Self {
            pending: Mutex::new(Vec::new()),
            on_change,
        }
    }

    pub fn ask(&self, view: QuestionView) -> oneshot::Receiver<Answer> {
        let (tx, rx) = oneshot::channel();
        let head = {
            let mut p = self.pending.lock();
            p.push(Pending { view, tx });
            p.first().map(|p| p.view.clone())
        };
        (self.on_change)(head);
        rx
    }

    /// The question shown now.
    pub fn head(&self) -> Option<QuestionView> {
        self.pending.lock().first().map(|p| p.view.clone())
    }

    pub fn get(&self, request_id: &str) -> Option<QuestionView> {
        self.pending
            .lock()
            .iter()
            .find(|p| p.view.request_id == request_id)
            .map(|p| p.view.clone())
    }

    /// A spoken answer, naming the request it answers.
    pub fn answer_by_voice(&self, request_id: &str, yes: bool) -> Result<(), ConfirmError> {
        let review = self.get(request_id).ok_or(ConfirmError::NotFound)?.review;
        if yes && review == Review::ScreenOnly {
            return Err(ConfirmError::NeedsScreen);
        }
        self.resolve(
            request_id,
            if yes { Answer::YesByVoice } else { Answer::No },
        )
    }

    /// A click on the HUD's Yes or No, from the main window.
    pub fn answer_on_screen(
        &self,
        request_id: &str,
        yes: Option<LocalReview>,
    ) -> Result<(), ConfirmError> {
        self.resolve(
            request_id,
            match yes {
                Some(review) => Answer::YesOnScreen(review),
                None => Answer::No,
            },
        )
    }

    fn resolve(&self, request_id: &str, answer: Answer) -> Result<(), ConfirmError> {
        let (found, head) = {
            let mut p = self.pending.lock();
            let i = p
                .iter()
                .position(|p| p.view.request_id == request_id)
                .ok_or(ConfirmError::NotFound)?;
            let found = p.remove(i);
            (found, p.first().map(|p| p.view.clone()))
        };
        let _ = found.tx.send(answer);
        (self.on_change)(head);
        Ok(())
    }

    /// Withdraws one question (it expired, or its action was cancelled).
    pub fn withdraw(&self, request_id: &str) {
        let _ = self.resolve(request_id, Answer::Cancelled);
    }

    /// The call ended: every question is cancelled.
    pub fn cancel_all(&self) {
        let all: Vec<Pending> = self.pending.lock().drain(..).collect();
        if all.is_empty() {
            return;
        }
        for p in all {
            let _ = p.tx.send(Answer::Cancelled);
        }
        (self.on_change)(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::origin::Origin;

    fn q(id: &str, review: Review) -> QuestionView {
        QuestionView {
            request_id: id.into(),
            tool: "close_pane".into(),
            summary: format!("question {id}"),
            verb: "close it".into(),
            review,
        }
    }

    fn confirms() -> (Confirms, Arc<Mutex<Vec<Option<String>>>>) {
        let shown = Arc::new(Mutex::new(Vec::new()));
        let s = shown.clone();
        (
            Confirms::new(Arc::new(move |h: Option<QuestionView>| {
                s.lock().push(h.map(|h| h.request_id))
            })),
            shown,
        )
    }

    #[tokio::test]
    async fn f_cn_03_2_a_voice_yes_for_a_resolves_a_and_never_b() {
        let (c, shown) = confirms();
        let mut a = c.ask(q("A", Review::VoiceOrScreen));
        let mut b = c.ask(q("B", Review::VoiceOrScreen));
        assert_eq!(c.head().map(|h| h.request_id), Some("A".into()));
        c.answer_by_voice("A", true).expect("A");
        assert!(matches!(a.try_recv(), Ok(Answer::YesByVoice)));
        // B is still waiting, and is now the one shown.
        assert!(b.try_recv().is_err());
        assert_eq!(c.head().map(|h| h.request_id), Some("B".into()));
        // A second "yes" for A answers nothing.
        assert_eq!(c.answer_by_voice("A", true), Err(ConfirmError::NotFound));
        assert!(b.try_recv().is_err());
        assert_eq!(
            shown.lock().clone(),
            vec![Some("A".into()), Some("A".into()), Some("B".into())]
        );
    }

    #[tokio::test]
    async fn f_cn_02_3_a_shell_command_takes_only_a_click() {
        let (c, _) = confirms();
        let mut rx = c.ask(q("S", Review::ScreenOnly));
        assert_eq!(c.answer_by_voice("S", true), Err(ConfirmError::NeedsScreen));
        assert!(rx.try_recv().is_err());
        // "No" by voice is fine: declining needs no proof.
        let mut rx2 = c.ask(q("T", Review::ScreenOnly));
        c.answer_by_voice("T", false).expect("no");
        assert!(matches!(rx2.try_recv(), Ok(Answer::No)));
        let review = LocalReview::check(&Origin::main_window()).expect("main");
        c.answer_on_screen("S", Some(review)).expect("click");
        assert!(matches!(rx.try_recv(), Ok(Answer::YesOnScreen(_))));
    }

    #[tokio::test]
    async fn f_cn_01_2_hanging_up_cancels_every_question() {
        let (c, shown) = confirms();
        let mut a = c.ask(q("A", Review::VoiceOrScreen));
        c.cancel_all();
        assert!(matches!(a.try_recv(), Ok(Answer::Cancelled)));
        assert_eq!(shown.lock().last().cloned(), Some(None));
        assert_eq!(c.answer_by_voice("A", true), Err(ConfirmError::NotFound));
    }
}
