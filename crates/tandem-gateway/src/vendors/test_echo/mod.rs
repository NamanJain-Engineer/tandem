//! Test vendor modules (feature `test-vendors`; never in release builds):
//! `test_echo` answers with what it's given and has one tool of every risk
//! class, `test_upper` shouts it back and `test_down` is a vendor that can't
//! be reached, for "an unavailable plugin doesn't break the others"
//! (F-PL-01). `test_echo__charge` costs what [`set_price`] last said, so a
//! test can change the amount between approval and execution (F-PL-06 AC2).

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    AuthKind, CallCtx, Category, Cost, ModuleStatus, PinnedHost, RiskClass, ToolDef, ToolOutput,
    Upstream, VendorError, VendorManifest, VendorModule,
};

static PRICE: AtomicI64 = AtomicI64::new(500);
static CALLS: AtomicU64 = AtomicU64::new(0);

/// What `test_echo__charge` costs now, in cents.
pub fn set_price(cents: i64) {
    PRICE.store(cents, Ordering::SeqCst);
}

/// How many calls reached the test vendors' "upstream" (ran for real).
pub fn upstream_calls() -> u64 {
    CALLS.load(Ordering::SeqCst)
}

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

fn tool(name: &str, description: &str, risk_class: RiskClass) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: text_schema(),
        risk_class,
    }
}

impl TestModule {
    fn new(kind: Kind) -> Self {
        let (id, label, logo, description) = match kind {
            Kind::Echo => (
                "test_echo",
                "Test Echo",
                "Te",
                "Answers with the text it's given.",
            ),
            Kind::Upper => ("test_upper", "Test Upper", "Tu", "Answers in capitals."),
            Kind::Down => (
                "test_down",
                "Test Down",
                "Td",
                "A vendor that can't be reached.",
            ),
        };
        Self {
            kind,
            manifest: VendorManifest {
                id,
                label,
                category: Category::Dev,
                logo,
                description,
                auth: AuthKind::None,
                alt_auth: None,
                upstream: Upstream::Rest {
                    base: "http://127.0.0.1:9/".into(),
                },
                pinned: vec![PinnedHost::new("http", "127.0.0.1", 9, "/")],
                auth_hosts: Vec::new(),
                status: ModuleStatus::Ready,
                risk_rules: Vec::new(),
                target_keys: vec!["target"],
                key_hint: None,
                scopes: Vec::new(),
            },
        }
    }
}

#[async_trait]
impl VendorModule for TestModule {
    fn manifest(&self) -> &VendorManifest {
        &self.manifest
    }

    async fn tools(&self, _cx: &CallCtx<'_>) -> Result<Vec<ToolDef>, VendorError> {
        match self.kind {
            Kind::Echo => Ok(vec![
                tool("echo", "Returns the text unchanged.", RiskClass::Read),
                tool("reverse", "Returns the text backwards.", RiskClass::Read),
                tool("note", "Saves a note (a write).", RiskClass::Write),
                tool(
                    "announce",
                    "Posts the text publicly.",
                    RiskClass::ExternalPublish,
                ),
                ToolDef {
                    name: "charge".into(),
                    description: "Charges the current price.".into(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {"text": {"type": "string"}, "amount": {"type": "integer"}, "currency": {"type": "string"}},
                        "required": ["text", "amount", "currency"]
                    }),
                    risk_class: RiskClass::Spend,
                },
                tool("enrich", "Uses vendor credits.", RiskClass::CreditConsuming),
                tool("wipe", "Deletes everything.", RiskClass::Destructive),
                tool(
                    "mystery",
                    "Does something nobody classified.",
                    RiskClass::Unclassified,
                ),
            ]),
            Kind::Upper => Ok(vec![tool(
                "upper",
                "Returns the text in capitals.",
                RiskClass::Read,
            )]),
            Kind::Down => Err(VendorError::Unavailable(
                "Test Down can't be reached".into(),
            )),
        }
    }

    async fn pre_execute(
        &self,
        _cx: &CallCtx<'_>,
        tool: &str,
        args: &Value,
    ) -> Result<Option<Cost>, VendorError> {
        if self.kind == Kind::Echo && tool == "charge" {
            // The "server-side" price, which may have moved since the card.
            return Ok(Some(Cost {
                amount: PRICE.load(Ordering::SeqCst),
                currency: args
                    .get("currency")
                    .and_then(Value::as_str)
                    .unwrap_or("usd")
                    .to_ascii_lowercase(),
            }));
        }
        Ok(self.describe(tool, args).cost)
    }

    async fn call(
        &self,
        _cx: &CallCtx<'_>,
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
            (Kind::Echo, "note" | "announce" | "charge" | "enrich" | "wipe" | "mystery") => {
                format!("{tool} done: {text}")
            }
            (Kind::Upper, "upper") => text.to_uppercase(),
            _ => return Err(VendorError::UnknownTool(tool.to_string())),
        };
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::ok(out))
    }
}
