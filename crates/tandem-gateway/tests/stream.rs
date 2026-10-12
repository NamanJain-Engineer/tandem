//! D-1604: an MCP session served over a byte stream (`serve_io`), as the
//! desktop does for an engine on an SSH host: bound to a lease held on this
//! machine, the same tools and checks as the listener, and over once the
//! lease is revoked.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tandem_core::access::Access;
use tandem_core::ids::ThreadId;
use tandem_gateway::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct Deny;

#[async_trait]
impl ApprovalAsker for Deny {
    async fn ask(&self, _caller: &Caller, _ask: ToolApproval) -> ApprovalAnswer {
        ApprovalAnswer::Deny {
            message: Some("asked on this machine".into()),
        }
    }
}

struct Lines {
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    read: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
}

impl Lines {
    async fn send(&mut self, v: Value) {
        let mut line = v.to_string();
        line.push('\n');
        self.write.write_all(line.as_bytes()).await.expect("write");
    }

    /// The reply to request `id`.
    async fn reply(&mut self, id: u64) -> Value {
        loop {
            let line = tokio::time::timeout(Duration::from_secs(10), self.read.next_line())
                .await
                .expect("in time")
                .expect("read")
                .expect("a line");
            let v: Value = serde_json::from_str(&line).expect("json");
            if v.get("id") == Some(&json!(id)) {
                return v;
            }
        }
    }
}

#[tokio::test]
async fn f_rm_01_f_pl_01_a_session_over_a_stream_is_bound_to_its_lease() {
    let gw = Gateway::start(GatewayDeps::new(Arc::new(Deny)))
        .await
        .expect("gateway");
    let lease = gw.prepare(SessionDescriptor::new(
        Caller::Thread {
            thread_id: ThreadId::new(),
        },
        Attended::Interactive,
        Access::Edits,
        BuiltinSet::for_thread(),
    ));
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (their_read, their_write) = tokio::io::split(theirs);
    let (our_read, our_write) = tokio::io::split(ours);
    let served = {
        let gw = gw.clone();
        let lease = Lease {
            id: lease.id.clone(),
            token: lease.token.clone(),
        };
        tokio::spawn(async move { gw.serve_io(&lease, their_read, their_write).await })
    };
    let mut c = Lines {
        write: our_write,
        read: BufReader::new(our_read).lines(),
    };
    c.send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26", "capabilities": {},
        "clientInfo": {"name": "engine-on-a-host", "version": "1"}}}),
    )
    .await;
    let init = c.reply(1).await;
    assert!(init.get("result").is_some(), "{init}");
    c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;
    c.send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;
    let listed = c.reply(2).await;
    let names: Vec<String> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();
    // A thread's lease: its built-ins and the approvals tool, as over HTTP.
    assert!(
        names.iter().any(|n| n.starts_with("tandem_approvals")),
        "{names:?}"
    );
    // An approval is asked on this machine.
    c.send(
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
        "name": names.iter().find(|n| n.starts_with("tandem_approvals")).expect("approvals"),
        "arguments": {"tool_name": "Bash", "input": {"command": "rm -rf /"}}}}),
    )
    .await;
    let asked = c.reply(3).await;
    assert!(
        asked.to_string().contains("asked on this machine"),
        "{asked}"
    );
    // Revoked: the stream gets nothing more from the lease.
    gw.revoke(&lease.id);
    c.send(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}))
        .await;
    let after = c.reply(4).await;
    assert!(after.get("error").is_some(), "{after}");
    assert!(after.to_string().contains("the lease ended"), "{after}");
    drop(c);
    tokio::time::timeout(Duration::from_secs(10), served)
        .await
        .expect("serve_io returns when the stream ends")
        .expect("join")
        .expect("clean end");
}
