//! A vendor reached through its REST API: the module declares each tool and
//! how it becomes a request under the pinned base URL.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{json, Value};
use url::Url;

use super::{
    AccountInfo, CallCtx, RiskClass, ToolDef, ToolOutput, Upstream, VendorError, VendorManifest,
    VendorModule,
};
use crate::pinned::Outgoing;

/// A request relative to the module's base URL.
#[derive(Debug, Clone, PartialEq)]
pub struct RestRequest {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<Value>,
}

impl RestRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            path: path.into(),
            query: Vec::new(),
            body: None,
        }
    }

    pub fn post(path: impl Into<String>, body: Value) -> Self {
        Self {
            method: Method::POST,
            path: path.into(),
            query: Vec::new(),
            body: Some(body),
        }
    }

    /// Adds `key=value` for each listed argument that is present.
    pub fn query_from(mut self, args: &Value, keys: &[&str]) -> Self {
        for k in keys {
            match args.get(*k) {
                Some(Value::String(s)) => self.query.push(((*k).to_string(), s.clone())),
                Some(Value::Number(n)) => self.query.push(((*k).to_string(), n.to_string())),
                Some(Value::Bool(b)) => self.query.push(((*k).to_string(), b.to_string())),
                _ => {}
            }
        }
        self
    }
}

pub type Build = fn(&Value) -> Result<RestRequest, VendorError>;
/// Reads the vendor's answer: `Err` when the body says it failed.
pub type Check = fn(u16, &Value) -> Result<(), String>;

pub struct RestTool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub class: RiskClass,
    pub build: Build,
}

pub struct RestModule {
    manifest: VendorManifest,
    tools: Vec<RestTool>,
    /// The cheap read `verify` makes, and where the account label is.
    whoami: (Build, &'static str),
    check: Check,
}

/// A required string argument.
pub fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, VendorError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| VendorError::Invalid(format!("{key} is required")))
}

/// Path segments are percent-encoded so an argument can't climb out.
pub fn seg(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>()
}

/// HTTP status decides.
pub fn check_status(status: u16, body: &Value) -> Result<(), String> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(body
            .pointer("/error/message")
            .or_else(|| body.get("error"))
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_else(|| format!("The server answered {status}")))
    }
}

impl RestModule {
    pub fn new(
        manifest: VendorManifest,
        tools: Vec<RestTool>,
        whoami: (Build, &'static str),
        check: Check,
    ) -> Self {
        Self {
            manifest,
            tools,
            whoami,
            check,
        }
    }

    fn base(&self) -> Result<Url, VendorError> {
        match &self.manifest.upstream {
            Upstream::Rest { base } => {
                Url::parse(base).map_err(|_| VendorError::Unavailable("bad base URL".into()))
            }
            _ => Err(VendorError::Unavailable("not a REST module".into())),
        }
    }

    async fn send(
        &self,
        cx: &CallCtx<'_>,
        req: RestRequest,
    ) -> Result<(u16, Value, String), VendorError> {
        let mut url = self
            .base()?
            .join(req.path.trim_start_matches('/'))
            .map_err(|_| VendorError::Invalid("bad path".into()))?;
        if !req.query.is_empty() {
            url.query_pairs_mut().extend_pairs(req.query.iter());
        }
        let out = match &req.body {
            Some(b) => Outgoing::json(req.method.clone(), url, b),
            None => Outgoing {
                method: req.method.clone(),
                ..Outgoing::get(url)
            },
        };
        let resp = cx.http.send(out).await?;
        let status = resp.status.as_u16();
        if status == 401 {
            return Err(VendorError::Unauthorized(
                "The server refused the sign-in (401)".into(),
            ));
        }
        let json = resp.json().unwrap_or(Value::Null);
        Ok((status, json, resp.body))
    }
}

#[async_trait]
impl VendorModule for RestModule {
    fn manifest(&self) -> &VendorManifest {
        &self.manifest
    }

    async fn tools(&self, _cx: &CallCtx<'_>) -> Result<Vec<ToolDef>, VendorError> {
        Ok(self
            .tools
            .iter()
            .map(|t| ToolDef {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema: (t.schema)(),
                risk_class: t.class,
            })
            .collect())
    }

    async fn call(
        &self,
        cx: &CallCtx<'_>,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput, VendorError> {
        let t = self
            .tools
            .iter()
            .find(|t| t.name == tool)
            .ok_or_else(|| VendorError::UnknownTool(tool.to_string()))?;
        let req = (t.build)(&args)?;
        let (status, json, body) = self.send(cx, req).await?;
        match (self.check)(status, &json) {
            Ok(()) => Ok(ToolOutput::ok(body)),
            Err(e) => Ok(ToolOutput::error(e)),
        }
    }

    async fn verify(&self, cx: &CallCtx<'_>) -> Result<AccountInfo, VendorError> {
        let (build, pointer) = self.whoami;
        let (status, json, _) = self.send(cx, build(&json!({}))?).await?;
        (self.check)(status, &json).map_err(VendorError::Invalid)?;
        Ok(AccountInfo {
            label: json
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(str::to_string),
            meta: Default::default(),
        })
    }
}
