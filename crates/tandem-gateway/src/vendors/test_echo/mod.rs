//! Test vendor modules (feature `test-vendors`; never in release builds):
//! `test_echo` answers with what it's given, `test_upper` shouts it back and
//! `test_down` is a vendor that can't be reached, for "an unavailable plugin
//! doesn't break the others" (F-PL-01).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    AuthKind, Category, ConnectionInfo, ModuleStatus, PinnedHost, RiskClass, ToolDef, ToolOutput,
    VendorError, VendorManifest, VendorModule,
};

pub fn modules() -> Vec<Arc<dyn VendorModule>> {
    vec![
        Arc::new(TestModule::new(Kind::Echo)),
        Arc::new(TestModule::new(Kind::Upper)),
        Arc::new(TestModule::new(Kind::Down)),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Echo,
    Upper,
    Down,
}

struct TestModule {
    kind: Kind,
    manifest: VendorManifest,
}

fn text_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"text": {"type": "string"}},
        "required": ["text"]
    })
}

impl TestModule {
    fn new(kind: Kind) -> Self {
        let (id, label, description) = match kind {
            Kind::Echo => (
                "test_echo",
                "Test Echo",
                "Answers with the text it's given.",
            ),
            Kind::Upper => ("test_upper", "Test Upper", "Answers in capitals."),
            Kind::Down => ("test_down", "Test Down", "A vendor that can't be reached."),
        };
        Self {
            kind,
            manifest: VendorManifest {
                id,
                label,
                category: Category::Dev,
                description,
                auth: AuthKind::None,
                pinned: vec![PinnedHost {
                    scheme: "http",
                    host: "127.0.0.1",
                    port: 9,
                    path_prefix: "/",
                }],
                status: ModuleStatus::Ready,
            },
        }
    }
}

#[async_trait]
impl VendorModule for TestModule {
    fn manifest(&self) -> &VendorManifest {
        &self.manifest
    }

    async fn tools(&self, _conn: &ConnectionInfo) -> Result<Vec<ToolDef>, VendorError> {
        match self.kind {
            Kind::Echo => Ok(vec![
                ToolDef {
                    name: "echo".into(),
                    description: "Returns the text unchanged.".into(),
                    input_schema: text_schema(),
                    risk_class: RiskClass::Read,
                },
                ToolDef {
                    name: "reverse".into(),
                    description: "Returns the text backwards.".into(),
                    input_schema: text_schema(),
                    risk_class: RiskClass::Read,
                },
            ]),
            Kind::Upper => Ok(vec![ToolDef {
                name: "upper".into(),
                description: "Returns the text in capitals.".into(),
                input_schema: text_schema(),
                risk_class: RiskClass::Read,
            }]),
            Kind::Down => Err(VendorError::Unavailable(
                "Test Down can't be reached".into(),
            )),
        }
    }

    async fn call(
        &self,
        conn: &ConnectionInfo,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput, VendorError> {
        if self.kind == Kind::Down {
            return Err(VendorError::Unavailable(
                "Test Down can't be reached".into(),
            ));
        }
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| VendorError::Invalid("text is required".into()))?;
        let out = match (self.kind, tool) {
            (Kind::Echo, "echo") => text.to_string(),
            (Kind::Echo, "reverse") => text.chars().rev().collect(),
            (Kind::Upper, "upper") => text.to_uppercase(),
            _ => return Err(VendorError::UnknownTool(tool.to_string())),
        };
        let _ = conn;
        Ok(ToolOutput {
            text: out,
            is_error: false,
        })
    }
}
