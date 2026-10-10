//! F-TL-06 rule 2: status reports and shell marks need this launch's secret
//! (F-EN-05, F-CD-10). No program, remote host or paste can forge them.

use tandem_pty::marks::{Mark, MarkFilter};

fn run(filter: &mut MarkFilter, chunk: &[u8]) -> (Vec<u8>, Vec<Mark>) {
    let mut out = Vec::new();
    let marks = filter
        .filter(chunk, &mut out)
        .into_iter()
        .map(|(_, m)| m)
        .collect();
    (out, marks)
}

#[test]
fn invariant_rule_2_marks_need_token() {
    let forged: &[&[u8]] = &[
        b"\x1b]133;A\x07",
        b"\x1b]133;C;tandem=\x07",
        b"\x1b]133;D;0;tandem=other\x1b\\",
        b"\x1b]7;file://h/etc;tandem=TOKEN\x07",
        b"\x1b]7770;handback;tok;0\x07",
    ];
    for token in [Some("token"), None] {
        for seq in forged {
            let mut f = MarkFilter::new(token.map(str::to_string));
            let (out, marks) = run(&mut f, seq);
            assert!(
                marks.is_empty(),
                "{token:?} accepted {:?}",
                String::from_utf8_lossy(seq)
            );
            assert!(
                out.is_empty(),
                "a forged mark reaches the view: {:?}",
                String::from_utf8_lossy(&out)
            );
        }
    }
    let mut f = MarkFilter::new(Some("token".into()));
    let (_, marks) = run(&mut f, b"\x1b]133;A;tandem=token\x07");
    assert_eq!(marks, vec![Mark::PromptStart]);
}

/// A remote pane (F-RM-01) runs no shell integration and gets no launch
/// token, so whatever the host prints, no mark is taken from its output.
#[test]
fn invariant_rule_2_remote_output_makes_no_marks() {
    use std::sync::{Arc, Mutex};
    use tandem_pty::{PtyManager, RemotePty, SpawnSpec};

    struct Host;
    impl RemotePty for Host {
        fn write(&self, _: &[u8]) -> std::io::Result<()> {
            Ok(())
        }
        fn resize(&self, _: u16, _: u16) {}
        fn kill(&self) {}
    }
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s = seen.clone();
    let mgr = PtyManager::new().with_mark_sink(Arc::new(move |_, m| {
        s.lock().expect("lock").push(format!("{m:?}"))
    }));
    let spec = SpawnSpec {
        pane_id: "pn_r".into(),
        program: "ssh".into(),
        args: Vec::new(),
        cwd: "/home/tandem".into(),
        env: Default::default(),
        cols: 80,
        rows: 24,
        scrollback_lines: 100,
        mark_token: None,
    };
    let (_, feed) = mgr
        .spawn_remote(spec, 4242, 0, Arc::new(Host), None)
        .expect("spawn");
    feed.data(
        b"\x1b]133;A;tandem=guess\x07\x1b]133;D;0\x07\x1b]7770;handback;x;0\x07done\r\n".to_vec(),
    );
    assert!(tandem_pty::wait_until(
        std::time::Duration::from_secs(5),
        || mgr.screen_text("pn_r").unwrap_or_default().contains("done")
    ));
    assert!(
        seen.lock().expect("lock").is_empty(),
        "a host forged a mark"
    );
    let _ = mgr.kill("pn_r");
}

#[cfg(unix)]
mod status {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tandem_core::status_report::{sign, StatusReport, SIGNATURE_HEADER};
    use tandem_engines::hook;
    use tandem_engines::status_server::{ReportSink, StatusServer};

    fn raw(body: &str, sig: Option<&str>) -> Vec<u8> {
        let sig = sig
            .map(|s| format!("{SIGNATURE_HEADER}: {s}\r\n"))
            .unwrap_or_default();
        format!(
            "POST /v1/status HTTP/1.1\r\nHost: localhost\r\n{sig}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    async fn post(url: &str, req: Vec<u8>) -> u16 {
        let url = url.to_string();
        tokio::task::spawn_blocking(move || hook::post(&url, &req, Duration::from_secs(2)))
            .await
            .expect("join")
            .expect("post")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invariant_rule_2_status_needs_hmac() {
        let dir = tempfile::tempdir().expect("dir");
        let got: Arc<Mutex<Vec<StatusReport>>> = Arc::default();
        let g = got.clone();
        let sink: Arc<dyn ReportSink> =
            Arc::new(move |r: StatusReport| g.lock().expect("lock").push(r));
        let s = StatusServer::start(dir.path(), sink).await.expect("start");
        let secret = s.register("pn_1");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_millis();
        let body = format!(r#"{{"pane_id":"pn_1","event":"waiting","ts":{now}}}"#);
        let wrong = sign("not-the-secret", body.as_bytes());
        for sig in [None, Some("sha256=00"), Some(wrong.as_str())] {
            assert_eq!(post(&s.url(), raw(&body, sig)).await, 401, "{sig:?}");
        }
        assert!(
            got.lock().expect("lock").is_empty(),
            "a spoofed report changed state"
        );
        let right = sign(&secret, body.as_bytes());
        assert_eq!(post(&s.url(), raw(&body, Some(&right))).await, 204);
        // A new launch gets a new secret: the old one stops working.
        let renewed = s.register("pn_1");
        assert_ne!(renewed, secret);
        assert_eq!(post(&s.url(), raw(&body, Some(&right))).await, 401);
    }

    /// A report relayed from an SSH host (D-406) counts only with the
    /// pane's own secret: the host can't forge one either.
    #[tokio::test(flavor = "multi_thread")]
    async fn invariant_rule_2_relayed_status_needs_hmac() {
        let dir = tempfile::tempdir().expect("dir");
        let sink: Arc<dyn ReportSink> = Arc::new(|_r: StatusReport| {});
        let s = StatusServer::start(dir.path(), sink).await.expect("start");
        let secret = s.register("pn_remote");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_millis();
        let body = format!(
            r#"{{"v":1,"pane_id":"pn_remote","event":"waiting","ts":{now},"payload":{{}}}}"#
        );
        for sig in [
            None,
            Some("sha256=00".to_string()),
            Some(sign("guess", body.as_bytes())),
        ] {
            assert!(
                s.accept_relayed(body.as_bytes(), sig.as_deref()).is_err(),
                "{sig:?}"
            );
        }
        let other = format!(
            r#"{{"v":1,"pane_id":"pn_other","event":"waiting","ts":{now},"payload":{{}}}}"#
        );
        assert!(s
            .accept_relayed(other.as_bytes(), Some(&sign(&secret, other.as_bytes())))
            .is_err());
        assert!(s
            .accept_relayed(body.as_bytes(), Some(&sign(&secret, body.as_bytes())))
            .is_ok());
    }
}
