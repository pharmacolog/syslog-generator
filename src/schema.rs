use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SchemaField {
    #[serde(rename = "type")]
    pub field_type: String,
    pub faker: Option<String>,
    pub values: Option<Vec<String>>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub format: Option<String>,
    /// F5: длина для type="string".
    pub len: Option<usize>,
    /// F5: джиттер в секундах для type="datetime" (now ± jitter). 0/None = точное «now».
    pub jitter_secs: Option<i64>,
    /// F6: распределение выбора для type="enum": "uniform" (дефолт) | "weighted" | "zipf".
    pub distribution: Option<String>,
    /// F6: веса для distribution="weighted" (длина = числу вариантов values).
    pub weights: Option<Vec<f64>>,
    /// F6: экспонента для distribution="zipf" (>0, дефолт 1.0).
    pub zipf_exponent: Option<f64>,
    /// F5: регулярное выражение для type="regex" (строка генерируется по паттерну).
    pub regex: Option<String>,
    /// F6 (межполевые корреляции): имя другого поля, от значения которого
    /// зависит это поле. Зависимое поле генерируется ПОСЛЕ родителя, а его
    /// значение выбирается из `mapping` по значению родителя.
    pub depends_on: Option<String>,
    /// F6: таблица соответствия «значение родителя → значение этого поля».
    /// Используется только если задан `depends_on`.
    pub mapping: Option<HashMap<String, String>>,
    /// F6: значение по умолчанию, если значение родителя отсутствует в `mapping`.
    /// Если не задано и совпадения нет — поле генерируется своим базовым типом.
    pub mapping_default: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Schema {
    #[serde(default)]
    pub fields: HashMap<String, SchemaField>,
    pub template: Option<String>,
    pub output: Option<String>,
}
