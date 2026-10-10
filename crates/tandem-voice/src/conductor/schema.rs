//! The doer's closed tool schema on this side (F-CN-02): the API checks
//! every call against `apps/api/voice-tools.json` before it reaches the
//! app, and the app checks again against the same file, so a call outside
//! the catalog or its types never runs, whatever the server sent.
//!
//! The check covers the JSON Schema keywords the catalog uses: `type`,
//! `properties`, `required`, `additionalProperties: false`, `enum`,
//! `anyOf`, `items`, string and array lengths and integer bounds.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

const CATALOG: &str = include_str!("../../../../apps/api/voice-tools.json");

fn schemas() -> &'static HashMap<String, Value> {
    static S: OnceLock<HashMap<String, Value>> = OnceLock::new();
    S.get_or_init(|| {
        let v: Value = serde_json::from_str(CATALOG).unwrap_or(Value::Null);
        v["tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|t| {
                        Some((t["name"].as_str()?.to_string(), t["input_schema"].clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Whether the doer's catalog has `name`.
pub fn known(name: &str) -> bool {
    schemas().contains_key(name)
}

/// The tool names in the doer's catalog.
pub fn names() -> Vec<String> {
    let mut v: Vec<String> = schemas().keys().cloned().collect();
    v.sort();
    v
}

/// Checks `args` against the tool's input schema. The error names the
/// argument, never its value.
pub fn check(name: &str, args: &Value) -> Result<(), String> {
    let schema = schemas()
        .get(name)
        .ok_or_else(|| format!("{name} isn't a Conductor tool"))?;
    matches(schema, args, "arguments")
}

fn matches(schema: &Value, v: &Value, at: &str) -> Result<(), String> {
    if let Some(any) = schema["anyOf"].as_array() {
        return if any.iter().any(|s| matches(s, v, at).is_ok()) {
            Ok(())
        } else {
            Err(format!("{at} has the wrong type"))
        };
    }
    if let Some(allowed) = schema["enum"].as_array() {
        if !allowed.contains(v) {
            return Err(format!("{at} isn't one of the allowed values"));
        }
    }
    let ty = schema["type"].as_str().unwrap_or("any");
    match ty {
        "object" => {
            let obj = v
                .as_object()
                .ok_or_else(|| format!("{at} isn't an object"))?;
            let props = schema["properties"].as_object();
            for req in schema["required"].as_array().into_iter().flatten() {
                let key = req.as_str().unwrap_or_default();
                if !obj.contains_key(key) {
                    return Err(format!("{key} is missing"));
                }
            }
            for (k, val) in obj {
                match props.and_then(|p| p.get(k)) {
                    Some(s) => matches(s, val, k)?,
                    None if schema["additionalProperties"] == Value::Bool(false) => {
                        return Err(format!("{k} isn't an argument of this tool"))
                    }
                    None => {}
                }
            }
            Ok(())
        }
        "string" => {
            let s = v.as_str().ok_or_else(|| format!("{at} isn't text"))?;
            let n = s.chars().count() as u64;
            if schema["minLength"].as_u64().is_some_and(|m| n < m) {
                return Err(format!("{at} is too short"));
            }
            if schema["maxLength"].as_u64().is_some_and(|m| n > m) {
                return Err(format!("{at} is too long"));
            }
            Ok(())
        }
        "integer" => {
            let i = v
                .as_i64()
                .ok_or_else(|| format!("{at} isn't a whole number"))?;
            if schema["minimum"].as_i64().is_some_and(|m| i < m)
                || schema["maximum"].as_i64().is_some_and(|m| i > m)
            {
                return Err(format!("{at} is out of range"));
            }
            Ok(())
        }
        "boolean" => v
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| format!("{at} isn't true or false")),
        "array" => {
            let items = v.as_array().ok_or_else(|| format!("{at} isn't a list"))?;
            let n = items.len() as u64;
            if schema["minItems"].as_u64().is_some_and(|m| n < m)
                || schema["maxItems"].as_u64().is_some_and(|m| n > m)
            {
                return Err(format!("{at} has the wrong number of items"));
            }
            for item in items {
                matches(&schema["items"], item, at)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_cn_02_the_catalog_is_embedded_with_every_tool() {
        assert_eq!(names().len(), tandem_core::voice_tools::catalog().len());
        for t in tandem_core::voice_tools::catalog() {
            assert!(known(t.name), "{}", t.name);
        }
    }

    #[test]
    fn f_cn_02_calls_outside_their_types_are_refused() {
        assert!(check("switch_mode", &json!({"mode": "code"})).is_ok());
        assert!(check("switch_mode", &json!({"mode": "shell"})).is_err());
        assert!(check("switch_mode", &json!({})).is_err());
        assert!(check("switch_mode", &json!({"mode": "code", "force": true})).is_err());
        assert!(check("read_pane", &json!({"pane": "p1", "lines": 201})).is_err());
        assert!(check("read_pane", &json!({"pane": "p1", "lines": 200})).is_ok());
        assert!(check("prompt_panes", &json!({"panes": [], "texts": ["x"]})).is_err());
        assert!(check(
            "prompt_pane",
            &json!({"pane": "p", "text": "go", "send": "yes"})
        )
        .is_err());
        assert!(check("set_setting", &json!({"key": "theme", "value": "dark"})).is_ok());
        assert!(check("set_setting", &json!({"key": "theme", "value": 3})).is_err());
        // F-CN-02 AC2: a key off the closed list.
        let err = check(
            "set_setting",
            &json!({"key": "telemetry_usage", "value": true}),
        )
        .expect_err("closed list");
        assert!(err.contains("key"), "{err}");
        assert!(check("rm_rf", &json!({})).is_err());
    }

    #[test]
    fn f_cn_02_errors_never_quote_values() {
        let err = check(
            "prompt_pane",
            &json!({"pane": "p", "text": "x".repeat(5000), "send": true}),
        )
        .expect_err("too long");
        assert!(!err.contains("xxx"), "{err}");
    }
}
