//! PR-B2 (Issue #83): --set точечные overrides для Profile.
//!
//! Поддерживает точечные JSON-path style keys:
//! - `targets[0].connections` — set field "connections" on first target
//! - `phases[0].messages_per_second=500000` — set field on first phase
//! - `distribution=round-robin` — set top-level field
//! - `metrics_addr=127.0.0.1:9090` — set top-level field
//!
//! Без внешних deps (используем serde_json::Value для навигации).
//!
//! ## Ограничения
//!
//! - Не поддерживает `target[?]` или `phases[-1]` syntax.
//! - Не поддерживает recursive merge — значение полностью перезаписывается.
//! - Path части (между `.`) должны существовать в Profile schema.

use crate::config::Profile;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;

/// Парсинг "path=value" → (path, value).
pub fn parse_set_entry(entry: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = entry.splitn(2, '=').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(anyhow!(
            "invalid --set entry {:?}: expected KEY=VALUE (non-empty KEY and VALUE)",
            entry
        ));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// Применить список --set overrides к Profile (in-place).
///
/// Парсит value как JSON; если не удаётся — fallback к String.
/// `targets[i].connections=8` парсит "8" как number.
/// `metrics_addr=127.0.0.1:9090` парсит как String.
pub fn apply_set_overrides(profile: &mut Profile, overrides: &[(String, String)]) -> Result<()> {
    // Конвертируем Profile → serde_json::Value, мутируем, конвертируем обратно.
    let mut value = serde_json::to_value(&*profile)
        .context("failed to serialize Profile to JSON for --set application")?;

    for (path, raw_value) in overrides {
        apply_set_to_value(&mut value, path, raw_value)?;
    }

    *profile = serde_json::from_value(value)
        .context("failed to deserialize Profile after --set application")?;
    Ok(())
}

/// Рекурсивно применить `path=value` к serde_json::Value.
///
/// Path формат: `a.b.c` для nested objects, `a[0].b` для массивов.
fn apply_set_to_value(root: &mut Value, path: &str, raw_value: &str) -> Result<()> {
    let parsed = parse_value(raw_value);
    let segments = parse_path(path)?;
    set_at_path(root, &segments, parsed)
}

/// Парсинг значения: сначала пробуем JSON (для чисел, bool, null),
/// fallback — String.
fn parse_value(s: &str) -> Value {
    // JSON парсинг — поддерживает numbers, booleans, null, arrays, objects.
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        return v;
    }
    // Fallback — String. Добавляем кавычки если нужно (для строк с спец. символами).
    Value::String(s.to_string())
}

/// Парсинг path: `a.b[0].c` → [`a`, `b`, `0`, `c`] (segments).
fn parse_path(path: &str) -> Result<Vec<PathSegment>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current.is_empty() {
                    segments.push(PathSegment::Field(std::mem::take(&mut current)));
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(PathSegment::Field(std::mem::take(&mut current)));
                }
                // Считываем число до ']'.
                let mut idx_str = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ']' {
                        chars.next();
                        break;
                    }
                    idx_str.push(next);
                    chars.next();
                }
                let idx: usize = idx_str
                    .parse()
                    .with_context(|| format!("invalid array index in path: {path}"))?;
                segments.push(PathSegment::Index(idx));
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        segments.push(PathSegment::Field(current));
    }
    if segments.is_empty() {
        return Err(anyhow!("empty path in --set: {:?}", path));
    }
    Ok(segments)
}

#[derive(Debug, Clone)]
enum PathSegment {
    Field(String),
    Index(usize),
}

fn set_at_path(value: &mut Value, segments: &[PathSegment], new: Value) -> Result<()> {
    if segments.is_empty() {
        *value = new;
        return Ok(());
    }
    match &segments[0] {
        PathSegment::Field(name) => {
            let obj = value
                .as_object_mut()
                .ok_or_else(|| anyhow!("path segment {:?}: target is not an object", name))?;
            let entry = obj.entry(name.clone()).or_insert_with(|| Value::Null);
            set_at_path(entry, &segments[1..], new)
        }
        PathSegment::Index(idx) => {
            let arr = value
                .as_array_mut()
                .ok_or_else(|| anyhow!("path segment [{}]: target is not an array", idx))?;
            if *idx >= arr.len() {
                return Err(anyhow!(
                    "array index {} out of bounds (length {})",
                    idx,
                    arr.len()
                ));
            }
            set_at_path(&mut arr[*idx], &segments[1..], new)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_entry_basic() {
        let (k, v) = parse_set_entry("targets[0].connections=8").unwrap();
        assert_eq!(k, "targets[0].connections");
        assert_eq!(v, "8");
    }

    #[test]
    fn parse_set_entry_rejects_invalid() {
        assert!(parse_set_entry("no_equals").is_err());
        assert!(parse_set_entry("=no_key").is_err());
        assert!(parse_set_entry("no_value=").is_err());
    }

    #[test]
    fn parse_path_simple() {
        let segs = parse_path("targets[0].connections").unwrap();
        assert_eq!(segs.len(), 3);
        assert!(matches!(&segs[0], PathSegment::Field(s) if s == "targets"));
        assert!(matches!(&segs[1], PathSegment::Index(0)));
        assert!(matches!(&segs[2], PathSegment::Field(s) if s == "connections"));
    }

    #[test]
    fn parse_value_number() {
        assert_eq!(parse_value("8"), Value::from(8));
        assert_eq!(parse_value("500000"), Value::from(500000));
    }

    #[test]
    fn parse_value_string() {
        assert_eq!(
            parse_value("127.0.0.1:9090"),
            Value::String("127.0.0.1:9090".to_string())
        );
    }

    #[test]
    fn apply_set_overrides_top_level_field() {
        let mut profile: Profile = serde_json::from_str(
            r#"{
                "targets": [],
                "distribution": "round-robin",
                "shutdown": {"mode": "drain", "drain_timeout_secs": 15},
                "phases": []
            }"#,
        )
        .unwrap();
        apply_set_overrides(&mut profile, &[("distribution".into(), "broadcast".into())]).unwrap();
        assert_eq!(profile.distribution, "broadcast");
    }

    #[test]
    fn apply_set_overrides_nested_array_index() {
        let mut profile: Profile = serde_json::from_str(
            r#"{
                "targets": [
                    {"address": "127.0.0.1:514", "transport": "tcp", "connections": 1, "weight": 1, "framing": "non-transparent"}
                ],
                "distribution": "round-robin",
                "shutdown": {"mode": "drain", "drain_timeout_secs": 15},
                "phases": []
            }"#,
        )
        .unwrap();
        apply_set_overrides(
            &mut profile,
            &[("targets[0].connections".into(), "8".into())],
        )
        .unwrap();
        assert_eq!(profile.targets[0].connections, 8);
    }

    #[test]
    fn apply_set_overrides_invalid_path() {
        let mut profile: Profile = serde_json::from_str(
            r#"{
                "targets": [],
                "distribution": "round-robin",
                "shutdown": {"mode": "drain", "drain_timeout_secs": 15},
                "phases": []
            }"#,
        )
        .unwrap();
        let res = apply_set_overrides(
            &mut profile,
            &[("nonexistent.deeply.nested".into(), "x".into())],
        );
        assert!(res.is_err());
    }
}
