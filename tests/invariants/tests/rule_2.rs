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
}
