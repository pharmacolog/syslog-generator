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
use anyhow::Result;

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
    /// Pre-compiled distribution sampler (placeholder для будущей реализации).
    pub distribution: CompiledDistribution,
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
    /// MVP: pre-sort fields by name, leave distribution/sampling для
    /// следующего шага (current behavior preserved).
    pub fn compile(schema: &Schema) -> Result<Self> {
        let mut fields: Vec<_> = schema
            .fields
            .iter()
            .map(|(name, field)| CompiledSchemaField {
                name: name.clone(),
                field_type: field.field_type.clone(),
                depends_on: field.depends_on.clone(),
                distribution: CompiledDistribution::Uniform, // placeholder
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

        Ok(Self {
            fields,
            body_template,
            field_slot_names,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaField;

    #[test]
    fn compile_empty_schema() {
        let schema = Schema::default();
        let plan = CompiledSchema::compile(&schema).expect("compile");
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

        let plan = CompiledSchema::compile(&schema).expect("compile");
        assert_eq!(plan.fields.len(), 2);
        assert_eq!(plan.fields[0].name, "a_field");
        assert_eq!(plan.fields[1].name, "z_field");
        assert_eq!(plan.field_slot_names, vec!["a_field", "z_field"]);
    }
}
