//! F-TK-01: the task store.

use std::sync::{Arc, Barrier};
use std::time::Duration;

use chrono::TimeZone;
use tandem_core::clock::MockClock;
use tandem_tasks::*;

fn store() -> (tempfile::TempDir, TaskStore) {
    let dir = tempfile::tempdir().expect("dir");
    let s = TaskStore::open(dir.path().join("tasks")).expect("open");
    (dir, s)
}

fn new_issue(s: &TaskStore, title: &str) -> Issue {
    s.save_issue(
        SaveIssue {
            title: Some(title.into()),
            ..Default::default()
        },
        &Writer::human(),
    )
    .expect("create")
}

#[test]
fn f_tk_01_ids_are_tan_n_and_files_are_issue_n() {
    let (_d, s) = store();
    let a = new_issue(&s, "First");
    let b = new_issue(&s, "Second");
    assert_eq!((a.id.as_str(), b.id.as_str()), ("TAN-1", "TAN-2"));
    assert_eq!(a.revision, 1);
    assert_eq!(a.status, IssueStatus::Todo);
    assert!(s.dir().join("issues/ISSUE-2.json").exists());
    assert!(s.dir().join("meta.json").exists());
    assert_eq!(s.meta().expect("meta").next_number, 3);
    assert_eq!(s.get("TAN-2").expect("get").title, "Second");
    assert!(matches!(s.get("TAN-9"), Err(TaskError::NotFound(_))));
    assert!(matches!(s.get("TAN-01"), Err(TaskError::Invalid(_))));
    assert!(matches!(s.get("../x"), Err(TaskError::Invalid(_))));
}

#[test]
fn f_tk_01_1_concurrent_saves_at_one_revision_one_conflicts() {
    for _ in 0..10 {
        let (_d, s) = store();
        let issue = new_issue(&s, "Race");
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["in_progress", "done"]
            .into_iter()
            .map(|status| {
                let (s, barrier) = (s.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    s.save_issue(
                        SaveIssue {
                            id: Some("TAN-1".into()),
                            status: IssueStatus::parse(status),
                            expected_revision: Some(1),
                            ..Default::default()
                        },
                        &Writer::agent("me", format!("tc_{status}")),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("join"))
            .collect();
        let ok = results.iter().filter(|r| r.is_ok()).count();
        let conflicts: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
        assert_eq!(ok, 1, "exactly one save wins");
        assert_eq!(conflicts.len(), 1);
        match conflicts[0] {
            TaskError::RevisionConflict {
                current_revision, ..
            } => assert_eq!(*current_revision, 2),
            other => panic!("expected REVISION_CONFLICT, got {other:?}"),
        }
        assert_eq!(conflicts[0].code(), TaskErrorCode::RevisionConflict);
        assert_eq!(s.get(&issue.id).expect("get").revision, 2);
    }
}

#[test]
fn f_tk_01_1_an_update_needs_the_expected_revision() {
    let (_d, s) = store();
    new_issue(&s, "A");
    let err = s
        .save_issue(
            SaveIssue {
                id: Some("TAN-1".into()),
                title: Some("B".into()),
                ..Default::default()
            },
            &Writer::human(),
        )
        .expect_err("needs a revision");
    assert_eq!(err.code(), TaskErrorCode::Invalid);
}

#[test]
fn f_tk_01_2_an_idempotency_key_returns_the_first_result_for_24_hours() {
    let dir = tempfile::tempdir().expect("dir");
    let start = chrono::Utc
        .with_ymd_and_hms(2026, 3, 1, 9, 0, 0)
        .single()
        .expect("t");
    let clock = MockClock::new(start);
    let s = TaskStore::open_with_clock(dir.path(), Arc::new(clock.clone())).expect("open");
    let req = SaveIssue {
        title: Some("Once".into()),
        idempotency_key: Some("k-1".into()),
        ..Default::default()
    };
    let first = s.save_issue(req.clone(), &Writer::human()).expect("first");
    clock.advance(Duration::from_secs(23 * 3600));
    let again = s.save_issue(req.clone(), &Writer::human()).expect("again");
    assert_eq!(first, again, "the recorded result comes back");
    assert_eq!(s.all().expect("all").len(), 1, "nothing written again");
    assert_eq!(s.history("TAN-1").expect("history").len(), 1);
    // Comments too.
    let c1 = s
        .add_comment("TAN-1", "hello", None, Some("c-1"), &Writer::human())
        .expect("comment");
    let c2 = s
        .add_comment("TAN-1", "hello", None, Some("c-1"), &Writer::human())
        .expect("comment again");
    assert_eq!(c1, c2);
    assert_eq!(s.comments("TAN-1").expect("comments").len(), 1);
    // After the window the key is forgotten.
    clock.advance(Duration::from_secs(2 * 3600));
    let later = s.save_issue(req, &Writer::human()).expect("later");
    assert_eq!(later.id, "TAN-2");
    // And reopening compacts the old entries away.
    clock.advance(Duration::from_secs(25 * 3600));
    let _ = TaskStore::open_with_clock(dir.path(), Arc::new(clock.clone())).expect("reopen");
    let log = std::fs::read_to_string(dir.path().join("idempotency.jsonl")).expect("log");
    assert!(
        log.trim().is_empty(),
        "entries older than 24 h are dropped: {log}"
    );
}

#[test]
fn f_tk_01_3_history_records_every_status_change_append_only() {
    let (_d, s) = store();
    new_issue(&s, "Track me");
    let path = s.dir().join("issues/ISSUE-1.history.jsonl");
    let mut previous = std::fs::read(&path).expect("history");
    let mut rev = 1;
    for status in ["in_progress", "in_review", "done", "todo"] {
        let i = s
            .save_issue(
                SaveIssue {
                    id: Some("TAN-1".into()),
                    status: IssueStatus::parse(status),
                    expected_revision: Some(rev),
                    ..Default::default()
                },
                &Writer::agent("me", "tc_x"),
            )
            .expect("save");
        rev = i.revision;
        let now = std::fs::read(&path).expect("history");
        assert!(
            now.starts_with(&previous) && now.len() > previous.len(),
            "history only grows"
        );
        previous = now;
    }
    let statuses: Vec<_> = s
        .history("TAN-1")
        .expect("history")
        .into_iter()
        .filter(|e| e.field == "status")
        .map(|e| (e.from, e.to, e.connection))
        .collect();
    assert_eq!(statuses.len(), 4);
    assert_eq!(statuses[0].0, "todo");
    assert_eq!(statuses[0].1, "in_progress");
    assert_eq!(statuses[3].1, "todo");
    assert_eq!(statuses[3].2.as_deref(), Some("tc_x"));
    // A save that changes nothing writes nothing.
    let same = s
        .save_issue(
            SaveIssue {
                id: Some("TAN-1".into()),
                status: Some(IssueStatus::Todo),
                expected_revision: Some(rev),
                ..Default::default()
            },
            &Writer::human(),
        )
        .expect("noop");
    assert_eq!(same.revision, rev);
    assert_eq!(std::fs::read(&path).expect("h"), previous);
}

#[test]
fn f_tk_01_fields_are_validated_and_refs_checked() {
    let (_d, s) = store();
    let bad = |req: SaveIssue| {
        s.save_issue(req, &Writer::human())
            .expect_err("invalid")
            .code()
    };
    assert_eq!(bad(SaveIssue::default()), TaskErrorCode::Invalid);
    assert_eq!(
        bad(SaveIssue {
            title: Some("x".repeat(201)),
            ..Default::default()
        }),
        TaskErrorCode::Invalid
    );
    assert_eq!(
        bad(SaveIssue {
            title: Some("p".into()),
            priority: Some(5),
            ..Default::default()
        }),
        TaskErrorCode::Invalid
    );
    assert_eq!(
        bad(SaveIssue {
            title: Some("l".into()),
            labels: Some(vec!["lbl_nope".into()]),
            ..Default::default()
        }),
        TaskErrorCode::Invalid
    );
    let label = s.save_label(None, "bug", Some("#ef4444")).expect("label");
    let project = s.save_project(None, "Web", Some("WEB")).expect("project");
    let i = s
        .save_issue(
            SaveIssue {
                title: Some("ok".into()),
                labels: Some(vec![label.id.clone()]),
                project: Some(Some(project.id.clone())),
                assignee: Some(Some("me".into())),
                ..Default::default()
            },
            &Writer::human(),
        )
        .expect("create");
    assert_eq!(i.labels, vec![label.id]);
    // null clears the assignee.
    let req: SaveIssue = serde_json::from_value(serde_json::json!({
        "id": i.id, "assignee": null, "expected_revision": 1
    }))
    .expect("parse");
    let cleared = s.save_issue(req, &Writer::human()).expect("clear");
    assert_eq!(cleared.assignee, None);
    assert!(s.save_label(None, "Bug", None).is_err(), "names are unique");
    assert!(s.save_label(None, "x", Some("red")).is_err());
}

#[test]
fn f_tk_01_archive_restore_and_attempts() {
    let (_d, s) = store();
    new_issue(&s, "A");
    let a = s
        .set_archived("TAN-1", true, Some(1), &Writer::human())
        .expect("archive");
    assert!(a.archived);
    assert!(s
        .list(&IssueFilter::default(), None, None)
        .expect("list")
        .items
        .is_empty());
    let all = s
        .list(
            &IssueFilter {
                include_archived: Some(true),
                ..Default::default()
            },
            None,
            None,
        )
        .expect("list");
    assert_eq!(all.items.len(), 1);
    assert!(matches!(
        s.set_archived("TAN-1", false, Some(1), &Writer::human()),
        Err(TaskError::RevisionConflict { .. })
    ));
    s.set_archived("TAN-1", false, Some(2), &Writer::human())
        .expect("restore");
    let rec = AttemptRecord {
        id: "atm_1".into(),
        started_at: 1,
        ended_at: None,
        state: "running".into(),
        lane: None,
        summary: None,
    };
    s.put_attempt("TAN-1", rec.clone(), &Writer::human())
        .expect("attempt");
    let done = s
        .put_attempt(
            "TAN-1",
            AttemptRecord {
                state: "done".into(),
                ..rec
            },
            &Writer::human(),
        )
        .expect("attempt");
    assert_eq!(done.attempts.len(), 1);
    assert_eq!(done.attempts[0].state, "done");
}

#[test]
fn f_tk_01_comments_thread_and_connections_revoke() {
    let (_d, s) = store();
    new_issue(&s, "A");
    let root = s
        .add_comment("TAN-1", "root", None, None, &Writer::human())
        .expect("root");
    let reply = s
        .add_comment("TAN-1", "reply", Some(&root.id), None, &Writer::human())
        .expect("reply");
    assert_eq!(reply.parent_id.as_deref(), Some(root.id.as_str()));
    assert!(s
        .add_comment("TAN-1", "x", Some("cmt_nope"), None, &Writer::human())
        .is_err());
    assert!(matches!(
        s.add_comment("TAN-7", "x", None, None, &Writer::human()),
        Err(TaskError::NotFound(_))
    ));
    let c = s.create_connection("Thread A", "me").expect("conn");
    assert_eq!(s.connection(&c.id).expect("live").actor, "me");
    s.revoke_connection(&c.id).expect("revoke");
    assert!(matches!(
        s.connection(&c.id),
        Err(TaskError::ConnectionRevoked)
    ));
    assert!(matches!(
        s.connection("tc_unknown"),
        Err(TaskError::ConnectionRevoked)
    ));
}

#[test]
fn f_tk_01_filters_and_text_search() {
    let (_d, s) = store();
    new_issue(&s, "Fix login");
    new_issue(&s, "Write docs");
    s.save_issue(
        SaveIssue {
            id: Some("TAN-2".into()),
            status: Some(IssueStatus::Done),
            expected_revision: Some(1),
            ..Default::default()
        },
        &Writer::human(),
    )
    .expect("done");
    let by_text = s
        .list(
            &IssueFilter {
                text: Some("LOGIN".into()),
                ..Default::default()
            },
            None,
            None,
        )
        .expect("list");
    assert_eq!(by_text.items.len(), 1);
    let done = s
        .list(
            &IssueFilter {
                status: Some(vec![IssueStatus::Done]),
                ..Default::default()
            },
            None,
            None,
        )
        .expect("list");
    assert_eq!(done.items[0].id, "TAN-2");
    let counts = s.counts_by_status().expect("counts");
    assert_eq!((counts["todo"], counts["done"]), (1, 1));
}

#[test]
fn f_tk_06_3_a_task_keeps_its_suggested_engine() {
    let (_d, s) = store();
    let created = s
        .save_issue(
            SaveIssue {
                title: Some("Add OAuth".into()),
                suggested_engine: Some(Some("codex".into())),
                ..Default::default()
            },
            &Writer::human(),
        )
        .expect("create");
    assert_eq!(created.suggested_engine.as_deref(), Some("codex"));
    // Files written before it read back without one.
    let plain = new_issue(&s, "Plain");
    assert_eq!(plain.suggested_engine, None);
    let text = std::fs::read_to_string(s.dir().join("issues/ISSUE-2.json")).expect("file");
    assert!(!text.contains("suggested_engine"), "left out when unset");
    let cleared = s
        .save_issue(
            SaveIssue {
                id: Some(created.id.clone()),
                suggested_engine: Some(None),
                expected_revision: Some(created.revision),
                ..Default::default()
            },
            &Writer::human(),
        )
        .expect("clear");
    assert_eq!(cleared.suggested_engine, None);
    let bad = s.save_issue(
        SaveIssue {
            title: Some("Bad".into()),
            suggested_engine: Some(Some("Not An Id".into())),
            ..Default::default()
        },
        &Writer::human(),
    );
    assert!(matches!(bad, Err(TaskError::Invalid(_))));
}
