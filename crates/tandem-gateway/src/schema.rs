//! Checks a held draft's edited arguments against its tool's input schema
//! (F-PL-08 "Edit & run (JSON editor with schema validation)"). The subset
//! tools use: `type`, `properties`, `required`, `enum`, `items`,
//! `additionalProperties: false`, `minimum`/`maximum`, `minLength`/
//! `maxLength`. Errors name the path.

use serde_json::Value;

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => true,
    }
}

fn at(path: &str) -> String {
    if path.is_empty() {
        "The arguments".into()
    } else {
        format!("`{path}`")
    }
}

fn check(schema: &Value, v: &Value, path: &str) -> Result<(), String> {
    let Some(s) = schema.as_object() else {
        return Ok(());
    };
    if let Some(t) = s.get("type") {
        let ok = match t {
            Value::String(t) => type_ok(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_ok(t, v)),
            _ => true,
        };
        if !ok {
            return Err(format!(
                "{} must be {}",
                at(path),
                t.to_string().trim_matches('"')
            ));
        }
    }
    if let Some(Value::Array(options)) = s.get("enum") {
        if !options.contains(v) {
            return Err(format!(
                "{} must be one of {}",
                at(path),
                Value::Array(options.clone())
            ));
        }
    }
    if let (Some(n), Some(min)) = (v.as_f64(), s.get("minimum").and_then(Value::as_f64)) {
        if n < min {
            return Err(format!("{} must be at least {min}", at(path)));
        }
    }
    if let (Some(n), Some(max)) = (v.as_f64(), s.get("maximum").and_then(Value::as_f64)) {
        if n > max {
            return Err(format!("{} must be at most {max}", at(path)));
        }
    }
    if let Some(text) = v.as_str() {
        let len = text.chars().count() as u64;
        if s.get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| len < m)
        {
            return Err(format!("{} is too short", at(path)));
        }
        if s.get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| len > m)
        {
            return Err(format!("{} is too long", at(path)));
        }
    }
    if let Some(obj) = v.as_object() {
        if let Some(Value::Array(req)) = s.get("required") {
            for r in req.iter().filter_map(Value::as_str) {
                if !obj.contains_key(r) {
                    let p = if path.is_empty() {
                        r.to_string()
                    } else {
                        format!("{path}.{r}")
                    };
                    return Err(format!("`{p}` is required"));
                }
            }
        }
        let props = s.get("properties").and_then(Value::as_object);
        for (k, val) in obj {
            let p = if path.is_empty() {
                k.clone()
            } else {
                format!("{path}.{k}")
            };
            match props.and_then(|ps| ps.get(k)) {
                Some(sub) => check(sub, val, &p)?,
                None if s.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    return Err(format!("`{p}` isn't an argument of this tool"));
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(arr)) = (s.get("items"), v.as_array()) {
        for (i, item) in arr.iter().enumerate() {
            check(items, item, &format!("{path}[{i}]"))?;
        }
    }
    Ok(())
}

/// `Ok` when `value` fits `schema`; otherwise the first problem, in words.
pub fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    check(schema, value, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_pl_08_edited_arguments_are_checked_against_the_tools_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "owner": {"type": "string"},
                "title": {"type": "string", "minLength": 1},
                "labels": {"type": "array", "items": {"type": "string"}},
                "state": {"enum": ["open", "closed"]},
                "count": {"type": "integer", "minimum": 1}
            },
            "required": ["owner", "title"],
            "additionalProperties": false
        });
        assert!(validate(&schema, &json!({"owner": "o", "title": "t"})).is_ok());
        assert_eq!(
            validate(&schema, &json!({"owner": "o"})),
            Err("`title` is required".into())
        );
        assert_eq!(
            validate(&schema, &json!({"owner": 1, "title": "t"})),
            Err("`owner` must be string".into())
        );
        assert!(validate(&schema, &json!({"owner": "o", "title": ""})).is_err());
        assert!(validate(
            &schema,
            &json!({"owner": "o", "title": "t", "labels": ["a", 2]})
        )
        .is_err());
        assert!(validate(
            &schema,
            &json!({"owner": "o", "title": "t", "state": "merged"})
        )
        .is_err());
        assert!(validate(&schema, &json!({"owner": "o", "title": "t", "count": 0})).is_err());
        assert!(validate(&schema, &json!({"owner": "o", "title": "t", "extra": 1})).is_err());
        assert_eq!(
            validate(&schema, &json!([1])),
            Err("The arguments must be object".into())
        );
    }
}
