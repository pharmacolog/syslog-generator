//! PR-A2 (v10.8.0, step 2): pre-resolved schema execution plan.
//!
//! Текущая реализация (`src/generator/core.rs::generate_message_with_format`)
//! на каждом сообщении делает:
//! 1. `schema.fields.iter().collect::<Vec<_>>()` — heap alloc.
//! 2. `sort_by(|a, b| a.0.cmp(b.0))` — O(N log N) sort.
//! 3. Per field: `gen_schema_field(...)` + `values.insert(name.to_string(), ...)` —
//!    name clone + insert.
//! 4. Для regex: `regex_syntax::parse(pattern)` (heap alloc).
//! 5. Для Zipf: `Vec<f64>` allocation.
//!
//! PR-A2.2 plan: pre-compute всё это в `CompiledSchema` при compile-time.
//!
//! MVP этой итерации — только структуры. Полная реализация (с parsed
//! regex HIR, pre-sorted fields, pre-compiled samplers) — следующий PR.

use crate::schema::Schema;

/// Pre-compiled schema execution plan.
///
/// Заменяет runtime `schema.fields.iter().collect()` + sort на
/// pre-computed `Vec<CompiledSchemaField>`.
#[derive(Debug, Clone)]
pub struct CompiledSchema {
    /// Pre-sorted fields (by name, ascending). Гарантирует deterministic
    /// output при `seed`.
    pub fields: Vec<CompiledSchemaField>,
    /// Optional pre-compiled template (если schema задаёт `template` поле).
    pub body_template: Option<crate::plan::template::CompiledTemplateV2>,
    /// Slot names corresponding to `fields`. Caller должен populate
    /// arena в этом порядке.
    pub field_slot_names: Vec<String>,
}

/// Single schema field в compiled form.
#[derive(Debug, Clone)]
pub struct CompiledSchemaField {
    /// Field name (used for `values` map insertion — но в slot-based render
    /// используется `field_slot_names[idx]`).
    pub name: String,
    /// Field type ("enum", "int", "datetime", "string", "faker", "regex").
    pub field_type: String,
    /// Optional dependency on parent field (для F6 inter-field correlations).
    pub depends_on: Option<String>,
    /// Pre-compiled distribution sampler (PR-A2.5: cumulative weights для
    /// weighted/zipf). PR-A2.5 future: binary search на cumulative.
    pub distribution: CompiledDistribution,
    /// PR-A2.5: pre-resolved regex pattern (для type="regex" fields).
    /// Hot path: regex_syntax::parse(pattern) per-message заменяется на
    /// использование pre-parsed HIR. MVP: только паттерн сохранён,
    /// integration с gen_schema_field в PR-A2.6.
    pub regex_pattern: Option<String>,
}

/// Pre-compiled distribution sampler.
///
/// PR-A2.2 future work: для `weighted` заменить O(N) `weighted_index` на
/// pre-computed cumulative weights + binary search (O(log N)). Для `zipf`
/// — pre-computed `Vec<f64>` с one-time init.
#[derive(Debug, Clone)]
pub enum CompiledDistribution {
    Uniform,
    Weighted { cumulative: Vec<f64> },
    Zipf { cumulative: Vec<f64> },
}

impl CompiledSchema {
    /// Compile a Schema в CompiledSchema.
    ///
    /// PR-A2.5: pre-compile distributions (cumulative weights для weighted,
    /// cumulative weights для zipf) и cache parsed regex patterns.
    /// Pre-computation устраняет per-message allocation в gen_schema_field.
    ///
    /// Infallible: пустые/invalid schemas дают empty CompiledSchema.
    pub fn compile(schema: &Schema) -> Self {
        let mut fields: Vec<_> = schema
            .fields
            .iter()
            .map(|(name, field)| CompiledSchemaField {
                name: name.clone(),
                field_type: field.field_type.clone(),
                depends_on: field.depends_on.clone(),
                distribution: compile_distribution(field),
                regex_pattern: field.regex.clone(),
            })
            .collect();
        fields.sort_by(|a, b| a.name.cmp(&b.name));

        let field_slot_names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
        let body_template = schema.template.as_ref().map(|t| {
            let mut slots = Vec::new();
            crate::plan::template::CompiledTemplateV2::compile_from_strings(
                std::slice::from_ref(t),
                &mut slots,
            )
        });

        Self {
            fields,
            body_template,
            field_slot_names,
        }
    }
}

/// PR-A2.5: pre-compile distribution sampler для одного schema field.
/// MVP: weighted → cumulative weights; zipf → cumulative weights.
fn compile_distribution(field: &crate::schema::SchemaField) -> CompiledDistribution {
    match field.field_type.as_str() {
        "enum" => {
            // Pre-compute cumulative weights для weighted distribution.
            // PR-A0 (legacy) вызывал weighted_index() per-message с O(N) scan.
            // Pre-computed cumulative позволяет binary search O(log N).
            match field.distribution.as_deref() {
                Some("weighted") => {
                    if let Some(weights) = &field.weights {
                        return CompiledDistribution::Weighted {
                            cumulative: build_cumulative(weights),
                        };
                    }
                    CompiledDistribution::Uniform
                }
                Some("zipf") => {
                    // Zipf: генерируем cumulative weights на основе
                    // 1/k^exp для k ∈ [1..values.len()].
                    if let Some(values) = &field.values {
                        let n = values.len();
                        if n > 0 {
                            let exp = field.zipf_exponent.unwrap_or(1.0);
                            let raw: Vec<f64> =
                                (1..=n).map(|k| 1.0 / (k as f64).powf(exp)).collect();
                            return CompiledDistribution::Zipf {
                                cumulative: build_cumulative(&raw),
                            };
                        }
                    }
                    CompiledDistribution::Uniform
                }
                _ => CompiledDistribution::Uniform,
            }
        }
        _ => CompiledDistribution::Uniform,
    }
}

/// Build cumulative weights: cum[i] = sum(weights[0..=i]).
/// Используется для binary search при weighted/zipf sampling.
fn build_cumulative(weights: &[f64]) -> Vec<f64> {
    let mut cum = Vec::with_capacity(weights.len());
    let mut sum = 0.0;
    for &w in weights {
        if w > 0.0 {
            sum += w;
        }
        cum.push(sum);
    }
    // Normalize: total = sum of positive weights.
    // Binary search на cum будет: find first index where cum[i] >= r * total.
    cum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaField;

    #[test]
    fn compile_empty_schema() {
        let schema = Schema::default();
        let plan = CompiledSchema::compile(&schema);
        assert_eq!(plan.fields.len(), 0);
        assert!(plan.body_template.is_none());
    }

    #[test]
    fn compile_sorts_fields_by_name() {
        let mut schema = Schema::default();
        schema.fields.insert(
            "z_field".to_string(),
            SchemaField {
                field_type: "int".to_string(),
                faker: None,
                values: None,
                min: Some(0),
                max: Some(10),
                format: None,
                len: None,
                jitter_secs: None,
                distribution: None,
                weights: None,
                zipf_exponent: None,
                regex: None,
                depends_on: None,
                mapping: None,
                mapping_default: None,
            },
        );
        schema.fields.insert(
            "a_field".to_string(),
            SchemaField {
                field_type: "int".to_string(),
                faker: None,
                values: None,
                min: Some(0),
                max: Some(10),
                format: None,
                len: None,
                jitter_secs: None,
                distribution: None,
                weights: None,
                zipf_exponent: None,
                regex: None,
                depends_on: None,
                mapping: None,
                mapping_default: None,
            },
        );

        let plan = CompiledSchema::compile(&schema);
        assert_eq!(plan.fields.len(), 2);
        assert_eq!(plan.fields[0].name, "a_field");
        assert_eq!(plan.fields[1].name, "z_field");
        assert_eq!(plan.field_slot_names, vec!["a_field", "z_field"]);
    }

    /// PR-A2.5: pre-compile weighted distribution → cumulative weights.
    #[test]
    fn a2_5_precompile_weighted_distribution() {
        use std::collections::HashMap;
        let mut schema = Schema::default();
        schema.fields.insert(
            "level".to_string(),
            SchemaField {
                field_type: "enum".to_string(),
                values: Some(vec!["a".into(), "b".into(), "c".into()]),
                distribution: Some("weighted".to_string()),
                weights: Some(vec![1.0, 2.0, 7.0]),
                faker: None,
                min: None,
                max: None,
                format: None,
                len: None,
                jitter_secs: None,
                zipf_exponent: None,
                regex: None,
                depends_on: None,
                mapping: None,
                mapping_default: None,
            },
        );
        let plan = CompiledSchema::compile(&schema);
        assert_eq!(plan.fields.len(), 1);
        let dist = &plan.fields[0].distribution;
        match dist {
            CompiledDistribution::Weighted { cumulative } => {
                assert_eq!(cumulative, &vec![1.0, 3.0, 10.0]);
            }
            _ => panic!("expected Weighted, got {dist:?}"),
        }
    }

    /// PR-A2.5: pre-compile zipf distribution → cumulative weights.
    #[test]
    fn a2_5_precompile_zipf_distribution() {
        let mut schema = Schema::default();
        schema.fields.insert(
            "key".to_string(),
            SchemaField {
                field_type: "enum".to_string(),
                values: Some(vec!["k1".into(), "k2".into(), "k3".into(), "k4".into()]),
                distribution: Some("zipf".to_string()),
                zipf_exponent: Some(1.0),
                faker: None,
                min: None,
                max: None,
                format: None,
                len: None,
                jitter_secs: None,
                weights: None,
                regex: None,
                depends_on: None,
                mapping: None,
                mapping_default: None,
            },
        );
        let plan = CompiledSchema::compile(&schema);
        assert_eq!(plan.fields.len(), 1);
        match &plan.fields[0].distribution {
            CompiledDistribution::Zipf { cumulative } => {
                // 1/1 + 1/2 + 1/3 + 1/4 ≈ 2.0833.
                assert!((cumulative[3] - 2.0833).abs() < 0.01, "got: {cumulative:?}");
                assert_eq!(cumulative.len(), 4);
            }
            _ => panic!("expected Zipf"),
        }
    }

    /// PR-A2.5: regex pattern сохранён в CompiledSchemaField.
    #[test]
    fn a2_5_precompile_regex_pattern() {
        let mut schema = Schema::default();
        schema.fields.insert(
            "user_agent".to_string(),
            SchemaField {
                field_type: "regex".to_string(),
                regex: Some(r"[A-Z][a-z]+ \d+".to_string()),
                faker: None,
                values: None,
                min: None,
                max: None,
                format: None,
                len: None,
                jitter_secs: None,
                distribution: None,
                weights: None,
                zipf_exponent: None,
                depends_on: None,
                mapping: None,
                mapping_default: None,
            },
        );
        let plan = CompiledSchema::compile(&schema);
        assert_eq!(
            plan.fields[0].regex_pattern.as_deref(),
            Some(r"[A-Z][a-z]+ \d+")
        );
    }

    /// PR-A2.5: uniform distribution (по умолчанию) → Uniform.
    #[test]
    fn a2_5_uniform_distribution() {
        let mut schema = Schema::default();
        schema.fields.insert(
            "name".to_string(),
            SchemaField {
                field_type: "enum".to_string(),
                values: Some(vec!["a".into(), "b".into()]),
                distribution: Some("uniform".to_string()),
                faker: None,
                min: None,
                max: None,
                format: None,
                len: None,
                jitter_secs: None,
                weights: None,
                zipf_exponent: None,
                regex: None,
                depends_on: None,
                mapping: None,
                mapping_default: None,
            },
        );
        let plan = CompiledSchema::compile(&schema);
        assert!(matches!(
            plan.fields[0].distribution,
            CompiledDistribution::Uniform
        ));
    }
}
