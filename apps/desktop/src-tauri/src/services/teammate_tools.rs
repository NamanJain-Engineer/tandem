//! The gateway's teammate built-ins (ARCHITECTURE §11.2): each call is
//! scoped to the lease's teammate. `tandem__memory_write` (F-AG-07),
//! `tandem__read_skill` (F-AG-06, F-AG-09), `tandem__session_search`
//! (F-AG-11), `tandem__send_message` and `tandem__list_teammates` (F-AG-12).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tandem_gateway::{Builtin, LeaseContext, ToolOutput};
use tandem_store::memory::MemoryFile;

use crate::services::gateway::BuiltinService;
use crate::services::mail::{MailService, Outgoing, Recipient};
use crate::services::teammates::TeammatesService;

fn ok(text: impl Into<String>) -> ToolOutput {
    ToolOutput {
        text: text.into(),
        is_error: false,
    }
}

fn err(text: impl Into<String>) -> ToolOutput {
    ToolOutput {
        text: text.into(),
        is_error: true,
    }
}

pub struct TeammateTools {
    pub teammates: Arc<TeammatesService>,
    pub mail: Arc<MailService>,
}

#[async_trait]
impl BuiltinService for TeammateTools {
    async fn call(&self, lease: &LeaseContext, builtin: Builtin, args: Value) -> ToolOutput {
        let Some(teammate_id) = lease.session.caller.teammate() else {
            return err("This tool is for teammate chats");
        };
        let teammate = match self.teammates.get(teammate_id).await {
            Ok(t) => t,
            Err(e) => return err(e.message),
        };
        match builtin {
            Builtin::MemoryWrite => {
                let file = args
                    .get("file")
                    .and_then(Value::as_str)
                    .and_then(MemoryFile::parse);
                let markdown = args.get("markdown").and_then(Value::as_str);
                let (Some(file), Some(markdown)) = (file, markdown) else {
                    return err("file (\"notes\" or \"about\") and markdown are required");
                };
                match self.teammates.write_memory(&teammate, file, markdown).await {
                    Ok(_) => ok("Saved."),
                    Err(e) => err(e.message),
                }
            }
            Builtin::ReadSkill => {
                let Some(slug) = args.get("slug").and_then(Value::as_str) else {
                    return err("slug is required");
                };
                match self.teammates.read_skill_for_agent(&teammate, slug).await {
                    Ok(text) => ok(text),
                    Err(e) => err(e.message),
                }
            }
            Builtin::SessionSearch => {
                let Some(query) = args.get("query").and_then(Value::as_str) else {
                    return err("query is required");
                };
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .min(10) as u32;
                match tandem_store::repo::search::search(
                    self.teammates.store(),
                    &teammate.id,
                    query,
                    limit,
                )
                .await
                {
                    Ok(hits) => ok(serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())),
                    Err(e) => err(e.to_string()),
                }
            }
            Builtin::SendMessage => {
                let to = args.get("to").and_then(Value::as_str);
                let body = args.get("body").and_then(Value::as_str);
                let (Some(to), Some(body)) = (to, body) else {
                    return err("to and body are required");
                };
                let from_thread = match &lease.session.caller {
                    tandem_gateway::Caller::TeammateChat { thread_id, .. } => {
                        Some(thread_id.clone())
                    }
                    _ => None,
                };
                let sent = self
                    .mail
                    .send(Outgoing {
                        from: Some(teammate.id.clone()),
                        from_thread,
                        conversation_ref: Some(lease.session.conversation_ref.clone()),
                        to: Recipient::Name(to.to_string()),
                        body: body.to_string(),
                    })
                    .await;
                match sent {
                    Ok(row) if row.reason.is_some() => ok(format!(
                        "Queued for {}: mail is paused, so it's delivered when mail resumes.",
                        row.to
                    )),
                    Ok(row) => ok(format!("Sent to {}.", row.to)),
                    Err(e) => err(e.message),
                }
            }
            Builtin::ListTeammates => match self.mail.reachable(&teammate.id).await {
                Ok(list) => ok(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())),
                Err(e) => err(e.message),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::teammates::tests_support::{input, service};
    use tandem_core::access::Access;
    use tandem_core::ids::{LeaseId, ThreadId};
    use tandem_gateway::{Attended, BuiltinSet, Caller, SessionDescriptor};

    fn lease(teammate: &tandem_core::ids::TeammateId) -> LeaseContext {
        LeaseContext {
            id: LeaseId::new(),
            session: SessionDescriptor::new(
                Caller::TeammateChat {
                    teammate_id: teammate.clone(),
                    thread_id: ThreadId::new(),
                },
                Attended::Interactive,
                Access::Edits,
                BuiltinSet::for_teammate(true, false),
            ),
        }
    }

    #[tokio::test]
    async fn f_ag_07_1_tandem_memory_write_writes_the_leases_teammate_and_refuses_over_the_cap() {
        let (_d, svc) = service(None).await;
        let scout = svc.create(input("Scout", "claude")).await.expect("scout");
        let tools = TeammateTools {
            teammates: svc.clone(),
            mail: crate::services::mail::MailService::new(
                svc.store().clone(),
                tandem_core::bus::EventBus::new(),
                svc.clone(),
                Arc::new(crate::services::mail::tests_support::Host::default()),
            ),
        };
        let l = lease(&scout.id);
        let out = tools
            .call(
                &l,
                Builtin::MemoryWrite,
                serde_json::json!({"file": "notes", "markdown": "- uses pnpm\n"}),
            )
            .await;
        assert!(!out.is_error, "{}", out.text);
        assert_eq!(
            svc.memory(&scout.id).await.expect("m").notes,
            "- uses pnpm\n"
        );
        let over = tools
            .call(
                &l,
                Builtin::MemoryWrite,
                serde_json::json!({"file": "notes", "markdown": "y".repeat(5000)}),
            )
            .await;
        assert!(over.is_error);
        assert!(over.text.contains("Condense it"), "{}", over.text);
        assert_eq!(
            svc.memory(&scout.id).await.expect("m").notes,
            "- uses pnpm\n"
        );
        // A thread's lease has no teammate to write for.
        let mut thread = l.clone();
        thread.session.caller = Caller::Thread {
            thread_id: ThreadId::new(),
        };
        assert!(
            tools
                .call(&thread, Builtin::MemoryWrite, serde_json::json!({}))
                .await
                .is_error
        );
    }

    #[tokio::test]
    async fn f_ag_12_send_message_queues_mail_from_the_leases_teammate_and_list_teammates_names_the_others(
    ) {
        let (_d, svc) = service(None).await;
        let ada = svc.create(input("Ada", "claude")).await.expect("ada");
        svc.create(input("Bea", "claude")).await.expect("bea");
        let host = Arc::new(crate::services::mail::tests_support::Host::default());
        let mail = crate::services::mail::MailService::new(
            svc.store().clone(),
            tandem_core::bus::EventBus::new(),
            svc.clone(),
            host.clone(),
        );
        let tools = TeammateTools {
            teammates: svc.clone(),
            mail: mail.clone(),
        };
        let l = lease(&ada.id);
        let out = tools
            .call(
                &l,
                Builtin::SendMessage,
                serde_json::json!({"to": "Bea", "body": "CI is red"}),
            )
            .await;
        assert!(!out.is_error, "{}", out.text);
        assert_eq!(out.text, "Sent to Bea.");
        let rows = mail.list(&ada.id).await.expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].from, "Ada");
        let nobody = tools
            .call(
                &l,
                Builtin::SendMessage,
                serde_json::json!({"to": "Nobody", "body": "hi"}),
            )
            .await;
        assert!(nobody.is_error);
        let list = tools
            .call(&l, Builtin::ListTeammates, serde_json::json!({}))
            .await;
        assert!(!list.is_error, "{}", list.text);
        assert!(list.text.contains("\"name\":\"Bea\""), "{}", list.text);
        assert!(!list.text.contains("\"name\":\"Ada\""), "{}", list.text);
    }
}
