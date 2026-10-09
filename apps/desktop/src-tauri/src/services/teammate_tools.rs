//! The gateway's teammate built-ins (ARCHITECTURE §11.2): each call is
//! scoped to the lease's teammate. `tandem__memory_write` (F-AG-07),
//! `tandem__read_skill` (F-AG-06, F-AG-09); session search and mail join
//! with their tasks.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tandem_gateway::{Builtin, LeaseContext, ToolOutput};
use tandem_store::memory::MemoryFile;

use crate::services::gateway::BuiltinService;
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
            _ => err("This tool isn't available yet"),
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
}
