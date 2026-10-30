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

fn claimed_paths(s: &TaskStore, folder: &str) -> Vec<String> {
    s.claims()
        .expect("claims")
        .into_iter()
        .filter(|c| c.folder == folder)
        .map(|c| c.path)
        .collect()
}

#[test]
fn f_tk_05_1_a_folder_claimed_without_a_slash_covers_its_files() {
    let (_d, s) = store();
    let a = session(&s, "/r/app/.tandem/worktrees/add-oauth");
    let b = session(&s, "/r/app/.tandem/worktrees/fix-auth");
    s.claim(&a, &paths(&["src/auth", "src/authz.ts"]), "OAuth")
        .expect("a");
    let overlaps = s
        .claim(&b, &paths(&["src/auth/session.ts", "src/auth.ts"]), "fix")
        .expect("b");
    assert_eq!(overlaps.len(), 1, "{overlaps:?}");
    assert_eq!(
        overlaps[0].paths,
        vec!["src/auth/session.ts".to_string()],
        "src/auth covers what's inside it, not src/auth.ts or src/authz.ts"
    );
}

#[test]
fn f_tk_05_a_folder_with_or_without_its_slash_is_one_claim() {
    let (_d, s) = store();
    let a = session(&s, "/r/app");
    s.claim(&a, &paths(&["src/auth/"]), "first").expect("slash");
    s.claim(&a, &paths(&["src/auth"]), "second")
        .expect("no slash");
    let claims = s.claims().expect("claims");
    assert_eq!(claims.len(), 1, "{claims:?}");
    assert_eq!(
        (claims[0].path.as_str(), claims[0].intent.as_str()),
        ("src/auth", "second")
    );
}

#[test]
fn f_tk_05_releasing_a_folder_without_its_slash_gives_up_what_is_inside() {
    let (_d, s) = store();
    let a = session(&s, "/r/app");
    s.claim(
        &a,
        &paths(&["src/a.ts", "src/b/c.ts", "src", "srcx.ts"]),
        "",
    )
    .expect("a");
    assert_eq!(s.release(&a, Some(&paths(&["src"]))).expect("src"), 3);
    assert_eq!(claimed_paths(&s, "/r/app"), vec!["srcx.ts".to_string()]);
}

#[test]
fn f_tk_05_1_a_claim_takes_brackets_but_refuses_patterns() {
    let (_d, s) = store();
    let blog = session(&s, "/r/app/.tandem/worktrees/blog");
    let docs = session(&s, "/r/app/.tandem/worktrees/docs");
    // Brackets are part of real file names (a dynamic route).
    s.claim(
        &blog,
        &paths(&["apps/web/app/blog/[slug]/page.tsx"]),
        "the post page",
    )
    .expect("a file with brackets");
    let overlaps = s
        .claim(&docs, &paths(&["apps/web/app/blog/[slug]"]), "")
        .expect("a folder with brackets");
    assert_eq!(overlaps.len(), 1, "{overlaps:?}");
    assert_eq!(
        overlaps[0].paths,
        vec!["apps/web/app/blog/[slug]/page.tsx".to_string()]
    );
    // They match literally: `[slug]` isn't a set of letters.
    let overlaps = s
        .claim(&docs, &paths(&["apps/web/app/blog/s/page.tsx"]), "")
        .expect("another file");
    assert_eq!(
        overlaps[0].paths,
        vec!["apps/web/app/blog/[slug]/page.tsx".to_string()]
    );
    // `*` and `?` would be stored as names no file has.
    for pattern in ["src/*.ts", "src/?.ts", "**"] {
        match s.claim(&blog, &paths(&[pattern]), "") {
            Err(TaskError::Invalid(msg)) => {
                assert!(msg.contains("not a pattern"), "{pattern}: {msg}")
            }
            other => panic!("{pattern}: {other:?}"),
        }
    }
    assert!(matches!(
        s.release(&blog, Some(&paths(&["src/*"]))),
        Err(TaskError::Invalid(_))
    ));
}

#[test]
fn f_tk_05_2_an_edit_to_any_real_file_name_is_claimed() {
    let (_d, s) = store();
    let tree = "/r/app/.tandem/worktrees/one";
    let seen = ["app/[id]/page.tsx", "notes?.md", "a*b.txt"];
    s.claim_inferred(&scope(tree), &paths(&seen))
        .expect("real files, matched literally");
    assert_eq!(claimed_paths(&s, tree), seen);
    for real in seen {
        assert_eq!(
            tandem_tasks::store::normalize_relative_claim_path(real).expect(real),
            real
        );
    }
}

#[test]
fn f_tk_05_an_agents_paths_start_at_its_folder_or_are_absolute_inside_its_tree() {
    let (_d, s) = store();
    let tree = "/r/app/.tandem/worktrees/web";
    let web = s
        .create_connection_in(
            "Claude Code",
            "me",
            Some(ClaimScope {
                prefix: "packages/web".into(),
                ..scope(tree)
            }),
        )
        .expect("web")
        .id;
    s.claim(
        &web,
        &paths(&[
            "src/a.ts",
            "./README.md",
            "/r/app/.tandem/worktrees/web/packages/ui/b.ts",
        ]),
        "",
    )
    .expect("web");
    assert_eq!(
        claimed_paths(&s, tree),
        vec![
            "packages/web/src/a.ts".to_string(),
            "packages/web/README.md".to_string(),
            "packages/ui/b.ts".to_string(),
        ]
    );
    // Another worktree, its session at the top, names the same file.
    let top = session(&s, "/r/app/.tandem/worktrees/top");
    let overlaps = s
        .claim(&top, &paths(&["packages/web/src/a.ts"]), "")
        .expect("top");
    assert_eq!(overlaps[0].paths, vec!["packages/web/src/a.ts".to_string()]);
    // Release resolves paths the same way.
    assert_eq!(s.release(&web, Some(&paths(&["src"]))).expect("src"), 1);
    // An app-inferred path is relative to the tree's top, whatever the
    // session's folder.
    s.claim_inferred(
        &ClaimScope {
            prefix: "packages/web".into(),
            ..scope(tree)
        },
        &paths(&["packages/web/c.ts"]),
    )
    .expect("inferred");
    assert!(claimed_paths(&s, tree).contains(&"packages/web/c.ts".to_string()));
    for outside in [
        tree,
        "/r/app/.tandem/worktrees/web-2/a.ts",
        "/r/app/src/a.ts",
        "/etc/passwd",
        "C:/r/app/a.ts",
        "../ui/b.ts",
    ] {
        assert!(
            matches!(
                s.claim(&web, &paths(&[outside]), ""),
                Err(TaskError::Invalid(_))
            ),
            "{outside}"
        );
    }
}

#[test]
fn f_tk_05_paths_compare_case_folded_where_the_file_system_does() {
    let folds = tandem_core::contain::case_insensitive_fs();
    let (_d, s) = store();
    let one = "/r/app/.tandem/worktrees/one";
    let a = session(&s, one);
    s.claim(&a, &paths(&["src/Auth.ts"]), "first")
        .expect("as written");
    s.claim(&a, &paths(&["SRC/auth.ts"]), "second")
        .expect("another case");
    let claims = s.claims().expect("claims");
    if folds {
        assert_eq!(claims.len(), 1, "one file here: {claims:?}");
        assert_eq!(
            (claims[0].path.as_str(), claims[0].intent.as_str()),
            ("src/Auth.ts", "second"),
            "kept as first written"
        );
    } else {
        assert_eq!(claims.len(), 2, "two files here: {claims:?}");
    }
    let b = session(&s, "/r/app/.tandem/worktrees/two");
    let overlaps = s.claim(&b, &paths(&["Src"]), "").expect("b");
    if folds {
        assert_eq!(overlaps[0].paths, vec!["src/Auth.ts".to_string()]);
    } else {
        assert!(overlaps.is_empty(), "{overlaps:?}");
    }
    let released = s
        .release(&a, Some(&paths(&["src/AUTH.ts"])))
        .expect("release");
    assert_eq!(released, usize::from(folds));
}

#[test]
fn f_tk_05_release_refuses_over_50_paths_before_it_waits_for_the_lock() {
    let (_d, s) = store();
    let a = session(&s, "/r/app");
    s.claim(&a, &paths(&["a.ts"]), "").expect("a");
    let too_many: Vec<String> = (0..=CLAIM_PATHS_MAX).map(|i| format!("f{i}.ts")).collect();
    {
        // Another process is writing claims.
        let _held =
            tandem_tasks::lock::acquire(&s.dir().join("locks").join("claims.lock")).expect("held");
        let started = std::time::Instant::now();
        assert!(matches!(
            s.release(&a, Some(&too_many)),
            Err(TaskError::Invalid(_))
        ));
        assert!(matches!(
            s.release(&a, Some(&paths(&["src/*"]))),
            Err(TaskError::Invalid(_))
        ));
        assert!(
            started.elapsed() < tandem_tasks::lock::ACQUIRE_TIMEOUT,
            "refused without waiting for the lock"
        );
    }
    let fifty: Vec<String> = (1..CLAIM_PATHS_MAX)
        .map(|i| format!("f{i}.ts"))
        .chain(["a.ts".to_string()])
        .collect();
    assert_eq!(s.release(&a, Some(&fifty)).expect("50 paths"), 1);
}

/// `claims.json`'s file as it is now; a rewrite (temp, then rename) makes
/// a new one. `None` where there's no file, or no inode to tell by.
fn claims_file(s: &TaskStore) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(s.dir().join("claims.json"))
            .ok()
            .map(|m| m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = s;
        None
    }
}

#[test]
fn f_tk_05_3_stale_claims_are_pruned_by_what_keep_refuses() {
    let (_d, s) = store();
    // Nothing to prune or release: no claims file appears.
    assert_eq!(s.prune_claims(|_| false).expect("empty"), 0);
    assert_eq!(s.release_folder("/r/app").expect("empty"), 0);
    assert!(!s.dir().join("claims.json").exists());
    let main = session(&s, "/r/app");
    let one = session(&s, "/r/app/.tandem/worktrees/one");
    let gone = session(&s, "/r/app/.tandem/worktrees/gone");
    s.claim(&main, &paths(&["a.ts"]), "").expect("main");
    s.claim(&one, &paths(&["b.ts"]), "").expect("one");
    s.claim(&gone, &paths(&["c.ts", "d.ts"]), "").expect("gone");
    // A sweep: the main checkout's claims after a restart, and those of a
    // worktree deleted outside the app.
    let pruned = s
        .prune_claims(|c| !c.main && c.folder != "/r/app/.tandem/worktrees/gone")
        .expect("prune");
    assert_eq!(pruned, 3);
    let left: Vec<String> = s
        .claims()
        .expect("left")
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert_eq!(left, vec!["b.ts".to_string()]);
    // Keeping everything leaves the file as it was.
    let before = claims_file(&s);
    assert_eq!(s.prune_claims(|_| true).expect("none"), 0);
    assert_eq!(s.release_folder("/r/app").expect("none"), 0);
    assert_eq!(claims_file(&s), before, "claims.json isn't rewritten");
}

#[test]
fn f_tk_05_1_overlaps_come_from_within_each_repository_in_order() {
    let claim = |repo: &str, lane: &str, path: &str| Claim {
        path: path.into(),
        intent: String::new(),
        repo: repo.into(),
        folder: format!("{repo}/.tandem/worktrees/{lane}"),
        agent: lane.into(),
        connection: None,
        inferred: true,
        at: 0,
        main: false,
    };
    let claims = vec![
        claim("/r/b", "one", "x.ts"),
        claim("/r/a", "one", "x.ts"),
        claim("/r/c", "one", "x.ts"),
        claim("/r/b", "two", "src"),
        claim("/r/a", "two", "x.ts"),
        claim("/r/b", "two", "x.ts"),
        claim("/r/a", "three", "src"),
        claim("/r/b", "one", "src/b.ts"),
    ];
    let overlaps = tandem_tasks::store::overlaps_of(&claims);
    let seen: Vec<(&str, &[String; 2], &[String])> = overlaps
        .iter()
        .map(|o| (o.repo.as_str(), &o.agents, o.paths.as_slice()))
        .collect();
    let pair = ["one".to_string(), "two".to_string()];
    assert_eq!(
        seen,
        vec![
            ("/r/a", &pair, &["x.ts".to_string()][..]),
            (
                "/r/b",
                &pair,
                &["src/b.ts".to_string(), "x.ts".to_string()][..]
            ),
        ],
        "one per pair, never across repositories, in order"
    );
}

#[test]
fn f_wt_10_8_a_claim_made_for_a_working_tree_is_its_own_with_the_intent() {
    let (_d, s) = store();
    let tree = "/r/app/.tandem/worktrees/renamer";
    // From the top folder whatever a session's prefix, with the intent.
    let mut sub = scope(tree);
    sub.prefix = "src".into();
    let first = s
        .claim_for_tree(
            &sub,
            &paths(&["src/parse.ts"]),
            "Intent radar: renamed parse → parseLine, which tandem/user uses",
        )
        .expect("claim");
    assert!(first.is_empty());
    let claims = s.claims().expect("claims");
    assert_eq!(claims.len(), 1);
    let c = &claims[0];
    assert_eq!(c.path, "src/parse.ts");
    assert_eq!(c.agent, "renamer");
    assert!(!c.inferred && c.connection.is_none());
    assert_eq!(
        c.intent,
        "Intent radar: renamed parse → parseLine, which tandem/user uses"
    );
    // The sibling's agent claiming the file meets it.
    let user = session(&s, "/r/app/.tandem/worktrees/user");
    let overlaps = s
        .claim(&user, &paths(&["src/parse.ts"]), "use parse")
        .expect("user");
    assert_eq!(overlaps.len(), 1);
    assert_eq!(
        overlaps[0].agents,
        ["renamer".to_string(), "user".to_string()]
    );
    // Only the tree's own real files: no `..`, no absolute path.
    for bad in ["../x.ts", "/etc/passwd"] {
        assert!(
            s.claim_for_tree(&scope(tree), &paths(&[bad]), "x").is_err(),
            "{bad}"
        );
    }
    let long = "x".repeat(501);
    assert!(s
        .claim_for_tree(&scope(tree), &paths(&["a.ts"]), &long)
        .is_err());
}

#[test]
fn f_wt_10_8_a_tree_claim_keeps_real_names_and_an_agents_own_claim() {
    let (_d, s) = store();
    let tree = "/r/app/.tandem/worktrees/renamer";
    // Real files with `*` or `?` in their names: the agents' check refused
    // them as patterns.
    s.claim_for_tree(
        &scope(tree),
        &paths(&["src/what?.ts", "src/a*b.ts"]),
        "Intent radar: changed what",
    )
    .expect("claim");
    let mut names: Vec<String> = s
        .claims()
        .expect("claims")
        .into_iter()
        .map(|c| c.path)
        .collect();
    names.sort();
    assert_eq!(names, ["src/a*b.ts", "src/what?.ts"]);
    // An agent's own claim on a file stays as it said it.
    let me = session(&s, tree);
    s.claim(&me, &paths(&["src/own.ts"]), "my own words")
        .expect("own");
    s.claim_for_tree(&scope(tree), &paths(&["src/own.ts"]), "Intent radar: y")
        .expect("claim");
    let own = s
        .claims()
        .expect("claims")
        .into_iter()
        .find(|c| c.path == "src/own.ts")
        .expect("own");
    assert_eq!(own.intent, "my own words");
    assert_eq!(own.connection.as_deref(), Some(me.as_str()));
    assert!(!own.inferred);
    // A name the checks would refuse or change is left out, and the rest
    // are claimed: one failed the whole claim, or claimed another name (the
    // review of 30d5bcad).
    let tree2 = "/r/app/.tandem/worktrees/odd";
    let long = format!("src/{}.ts", "x".repeat(600));
    s.claim_for_tree(
        &scope(tree2),
        &paths(&[
            "src/ok.ts",
            " src/space.ts",
            "src\\back.ts",
            "src/c\u{7}.ts",
            long.as_str(),
        ]),
        "Intent radar: z",
    )
    .expect("the rest");
    let theirs: Vec<String> = s
        .claims()
        .expect("claims")
        .into_iter()
        .filter(|c| c.agent == "odd")
        .map(|c| c.path)
        .collect();
    assert_eq!(theirs, ["src/ok.ts"]);
    assert!(s
        .claim_for_tree(&scope(tree2), &paths(&[" src/space.ts"]), "x")
        .is_err());
}
