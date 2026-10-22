//! F-TK-05: claims in the task store.

use tandem_tasks::*;

fn store() -> (tempfile::TempDir, TaskStore) {
    let dir = tempfile::tempdir().expect("dir");
    let s = TaskStore::open(dir.path().join("tasks")).expect("open");
    (dir, s)
}

/// A working tree of `/r/app`, its session at the tree's top folder.
fn scope(folder: &str) -> ClaimScope {
    ClaimScope {
        repo: "/r/app".into(),
        folder: folder.into(),
        prefix: String::new(),
        main: folder == "/r/app",
    }
}

fn session(s: &TaskStore, folder: &str) -> String {
    s.create_connection_in("Claude Code", "me", Some(scope(folder)))
        .expect("connection")
        .id
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

#[test]
fn f_tk_05_1_overlapping_claims_make_one_overlap_naming_both_and_the_paths() {
    let (_d, s) = store();
    let a = session(&s, "/r/app/.tandem/worktrees/add-oauth");
    let b = session(&s, "/r/app/.tandem/worktrees/fix-auth");
    let first = s
        .claim(
            &a,
            &paths(&["src/auth/session.ts", "src/oauth.ts"]),
            "OAuth",
        )
        .expect("a");
    assert!(first.is_empty(), "nobody else claims these yet");
    let second = s
        .claim(&b, &paths(&["src/auth/", "README.md"]), "fix the auth bug")
        .expect("b");
    assert_eq!(second.len(), 1, "one overlap for the pair");
    let o = &second[0];
    assert_eq!(o.agents, ["add-oauth".to_string(), "fix-auth".to_string()]);
    assert_eq!(o.paths, vec!["src/auth/session.ts".to_string()]);
    assert_eq!(s.claim_overlaps().expect("all"), second);
}

#[test]
fn f_tk_05_1_one_working_tree_never_overlaps_itself_nor_another_repo() {
    let (_d, s) = store();
    let a = session(&s, "/r/app");
    let a2 = session(&s, "/r/app");
    s.claim(&a, &paths(&["a.ts"]), "").expect("a");
    assert!(s.claim(&a2, &paths(&["a.ts"]), "").expect("a2").is_empty());
    let other = s
        .create_connection_in(
            "Codex",
            "me",
            Some(ClaimScope {
                repo: "/r/other".into(),
                folder: "/r/other".into(),
                prefix: String::new(),
                main: true,
            }),
        )
        .expect("c")
        .id;
    assert!(s
        .claim(&other, &paths(&["a.ts"]), "")
        .expect("o")
        .is_empty());
}

#[test]
fn f_tk_05_2_an_inferred_claim_overlaps_and_keeps_a_stated_intent() {
    let (_d, s) = store();
    let a = session(&s, "/r/app/.tandem/worktrees/one");
    s.claim(&a, &paths(&["a.ts"]), "rename").expect("claim");
    s.claim_inferred(
        &scope("/r/app/.tandem/worktrees/one"),
        &paths(&["a.ts", "b.ts"]),
    )
    .expect("inferred");
    let overlaps = s
        .claim_inferred(&scope("/r/app/.tandem/worktrees/two"), &paths(&["b.ts"]))
        .expect("other");
    assert_eq!(overlaps[0].paths, vec!["b.ts".to_string()]);
    let claims = s.claims().expect("claims");
    let a_ts = claims.iter().find(|c| c.path == "a.ts").expect("a.ts");
    assert_eq!((a_ts.intent.as_str(), a_ts.inferred), ("rename", false));
    let b_ts = claims
        .iter()
        .find(|c| c.path == "b.ts" && c.agent == "one")
        .expect("b.ts");
    assert!(b_ts.inferred && b_ts.connection.is_none());
}

#[test]
fn f_tk_05_3_merging_or_removing_a_working_tree_releases_its_claims() {
    let (_d, s) = store();
    let a = session(&s, "/r/app/.tandem/worktrees/one");
    let b = session(&s, "/r/app/.tandem/worktrees/two");
    s.claim(&a, &paths(&["x.ts"]), "").expect("a");
    s.claim(&b, &paths(&["x.ts"]), "").expect("b");
    assert_eq!(
        s.release_folder("/r/app/.tandem/worktrees/one")
            .expect("rm"),
        1
    );
    assert!(s.claim_overlaps().expect("o").is_empty());
    assert_eq!(s.claims().expect("c").len(), 1);
}

#[test]
fn f_tk_05_release_gives_up_paths_or_everything() {
    let (_d, s) = store();
    let a = session(&s, "/r/app");
    s.claim(&a, &paths(&["src/a.ts", "src/b.ts", "c.ts"]), "")
        .expect("a");
    assert_eq!(s.release(&a, Some(&paths(&["src/"]))).expect("dir"), 2);
    assert_eq!(s.release(&a, None).expect("all"), 1);
    assert!(s.claims().expect("c").is_empty());
}

#[test]
fn f_tk_05_a_session_outside_a_working_tree_or_revoked_cant_claim() {
    let (_d, s) = store();
    let none = s.create_connection("Chat", "me").expect("c").id;
    assert!(matches!(
        s.claim(&none, &paths(&["a.ts"]), ""),
        Err(TaskError::Invalid(_))
    ));
    let a = session(&s, "/r/app");
    s.revoke_connection(&a).expect("revoke");
    assert!(matches!(
        s.claim(&a, &paths(&["a.ts"]), ""),
        Err(TaskError::ConnectionRevoked)
    ));
    let b = session(&s, "/r/app");
    let many: Vec<String> = (0..=CLAIM_PATHS_MAX).map(|i| format!("f{i}")).collect();
    assert!(s.claim(&b, &many, "").is_err());
    assert!(s
        .claim(&b, &paths(&["a"]), &"x".repeat(CLAIM_INTENT_MAX + 1))
        .is_err());
}

#[test]
fn f_tk_05_a_working_tree_over_its_cap_drops_its_oldest_inferred_claims() {
    let (_d, s) = store();
    let sc = scope("/r/app");
    let a = session(&s, "/r/app");
    s.claim(&a, &paths(&["kept.ts"]), "mine").expect("claim");
    for chunk in 0..(CLAIMS_PER_FOLDER_MAX / CLAIM_PATHS_MAX) {
        let batch: Vec<String> = (0..CLAIM_PATHS_MAX)
            .map(|i| format!("f{chunk}-{i}.ts"))
            .collect();
        s.claim_inferred(&sc, &batch).expect("batch");
    }
    let claims = s.claims().expect("c");
    assert_eq!(claims.len(), CLAIMS_PER_FOLDER_MAX);
    assert!(
        claims.iter().any(|c| c.path == "kept.ts"),
        "a stated claim stays"
    );
}

#[test]
fn f_tk_05_a_claim_copies_main_from_its_scope() {
    let (_d, s) = store();
    let main = session(&s, "/r/app");
    s.claim(&main, &paths(&["a.ts"]), "")
        .expect("main checkout");
    let lane = session(&s, "/r/app/.tandem/worktrees/one");
    s.claim(&lane, &paths(&["b.ts"]), "").expect("lane");
    s.claim_inferred(&scope("/r/app"), &paths(&["c.ts"]))
        .expect("inferred");
    let main_of = |path: &str| {
        s.claims()
            .expect("claims")
            .into_iter()
            .find(|c| c.path == path)
            .map(|c| c.main)
    };
    assert_eq!(main_of("a.ts"), Some(true));
    assert_eq!(main_of("b.ts"), Some(false));
    assert_eq!(main_of("c.ts"), Some(true), "an inferred claim too");
}

#[test]
fn f_tk_05_claims_and_scopes_written_before_main_and_prefix_still_load() {
    let (_d, s) = store();
    std::fs::write(
        s.dir().join("claims.json"),
        r#"[{"path":"src/a.ts","intent":"","repo":"/r/app","folder":"/r/app","agent":"app","connection":null,"inferred":true,"at":1}]"#,
    )
    .expect("old claims.json");
    std::fs::write(
        s.dir().join("connections.json"),
        r#"{"tc_old":{"id":"tc_old","created_at":1,"label":"Claude Code","actor":"me","revoked_at":null,"scope":{"repo":"/r/app","folder":"/r/app"}}}"#,
    )
    .expect("old connections.json");
    let claims = s.claims().expect("old claims load");
    assert_eq!(claims.len(), 1);
    assert!(!claims[0].main);
    let scope = s.claim_scope("tc_old").expect("an old scope loads");
    assert_eq!((scope.prefix.as_str(), scope.main), ("", false));
}
