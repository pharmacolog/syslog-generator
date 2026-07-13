//! Честная сериализация в protobuf wire-format (Protocol Buffers Encoding).
//!
//! Ранее режим `format: protobuf` фактически возвращал `serde_json::to_vec`,
//! что не является protobuf и не читается ни одним protobuf-приёмником. Здесь
//! реализован корректный wire-format согласно спецификации
//! <https://protobuf.dev/programming-guides/encoding/>:
//!
//! - Каждое поле кодируется как `tag = (field_number << 3) | wire_type`,
//!   записанный как varint, за которым следует значение.
//! - Строки/байты (`wire_type = 2`, LEN): `varint(len)` + сами байты.
//! - Целые (`wire_type = 0`, VARINT): значение как varint. Знаковые — zigzag
//!   при `sint`, иначе двоичное дополнение (как int64).
//! - Дробные (`wire_type = 1`/`5`, I64/I32): little-endian.
//! - Булевы: varint 0/1.
//!
//! Результат совместим с `protoc --decode_raw` и любым protobuf-парсером.

use crate::config::ProtobufSchemaFieldMap;
use crate::template::render_template;
use std::collections::HashMap;

/// Типы полей protobuf, поддерживаемые генератором.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbType {
    /// LEN, wire_type 2 — строка UTF-8.
    Str,
    /// LEN, wire_type 2 — произвольные байты (значение как строка UTF-8).
    Bytes,
    /// VARINT, wire_type 0 — int64 (двоичное дополнение для отрицательных).
    Int,
    /// VARINT, wire_type 0 — uint64.
    Uint,
    /// VARINT, wire_type 0 — sint64 (zigzag).
    Sint,
    /// VARINT, wire_type 0 — bool (0/1).
    Bool,
    /// I64, wire_type 1 — double (little-endian f64).
    Double,
    /// I32, wire_type 5 — float (little-endian f32).
    Float,
}

impl PbType {
    /// Разобрать тип из строкового имени (по умолчанию — строка).
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "int" | "int64" | "int32" => PbType::Int,
            "uint" | "uint64" | "uint32" => PbType::Uint,
            "sint" | "sint64" | "sint32" => PbType::Sint,
            "bool" | "boolean" => PbType::Bool,
            "double" | "f64" => PbType::Double,
            "float" | "f32" => PbType::Float,
            "bytes" => PbType::Bytes,
            _ => PbType::Str,
        }
    }

    /// Числовой код wire-type для этого типа.
    fn wire_type(self) -> u64 {
        match self {
            PbType::Str | PbType::Bytes => 2,
            PbType::Int | PbType::Uint | PbType::Sint | PbType::Bool => 0,
            PbType::Double => 1,
            PbType::Float => 5,
        }
    }
}

/// Записать беззнаковый varint (base-128, little-endian с continuation-битом).
pub fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// ZigZag-кодирование знакового int64 → uint64 (для sint).
fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Записать тег поля: (field_number << 3) | wire_type.
fn write_tag(buf: &mut Vec<u8>, field_number: u64, wire_type: u64) {
    write_varint(buf, (field_number << 3) | wire_type);
}

/// Закодировать одно поле в буфер согласно его типу.
fn encode_field(buf: &mut Vec<u8>, field_number: u64, ty: PbType, value: &str) {
    write_tag(buf, field_number, ty.wire_type());
    match ty {
        PbType::Str | PbType::Bytes => {
            let bytes = value.as_bytes();
            write_varint(buf, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }
        PbType::Int => {
            // int64: отрицательные — как u64 (двоичное дополнение), 10-байтовый varint.
            let v = value.trim().parse::<i64>().unwrap_or(0);
            write_varint(buf, v as u64);
        }
        PbType::Uint => {
            let v = value.trim().parse::<u64>().unwrap_or(0);
            write_varint(buf, v);
        }
        PbType::Sint => {
            let v = value.trim().parse::<i64>().unwrap_or(0);
            write_varint(buf, zigzag(v));
        }
        PbType::Bool => {
            let v = matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            );
            write_varint(buf, v as u64);
        }
        PbType::Double => {
            let v = value.trim().parse::<f64>().unwrap_or(0.0);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        PbType::Float => {
            let v = value.trim().parse::<f32>().unwrap_or(0.0);
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
}

/// Разобрать спецификацию поля вида `"name" -> "3:int:template"` или `"template"`.
///
/// Формат значения в `fields`:
/// - `"<field_number>:<type>:<template>"` — явные номер и тип поля;
/// - `"<field_number>:<template>"` — явный номер, тип по умолчанию `str`;
/// - `"<template>"` — номер назначается автоматически по порядку сортировки
///   имён (1..N), тип `str`. Обеспечивает обратную совместимость.
fn parse_field_spec(idx_default: u64, raw: &str) -> (u64, PbType, String) {
    // Пытаемся отделить префикс "<num>:" и опционально "<type>:".
    if let Some((head, rest)) = raw.split_once(':') {
        if let Ok(num) = head.trim().parse::<u64>() {
            // Есть номер. Проверяем, есть ли тип следующим сегментом.
            if let Some((maybe_type, tmpl)) = rest.split_once(':') {
                let ty = PbType::parse(maybe_type.trim());
                // Если сегмент не распознан как тип, PbType::parse вернёт Str,
                // но тогда двоеточие принадлежит шаблону. Различаем: тип валиден,
                // только если это одно из известных ключевых слов.
                if is_known_type(maybe_type.trim()) {
                    return (num, ty, tmpl.to_string());
                }
            }
            // Номер есть, тип по умолчанию, остаток — шаблон целиком.
            return (num, PbType::Str, rest.to_string());
        }
    }
    // Нет явного номера — авто-нумерация, строка.
    (idx_default, PbType::Str, raw.to_string())
}

/// Известно ли ключевое слово типа (для отделения типа от шаблона).
fn is_known_type(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "int"
            | "int64"
            | "int32"
            | "uint"
            | "uint64"
            | "uint32"
            | "sint"
            | "sint64"
            | "sint32"
            | "bool"
            | "boolean"
            | "double"
            | "f64"
            | "float"
            | "f32"
            | "bytes"
            | "str"
            | "string"
    )
}

/// Применить protobuf-схему: отрендерить шаблоны значений и вернуть
/// упорядоченный список (field_number, type, rendered_value).
///
/// Поля сортируются по имени для детерминированного порядка; авто-номера
/// назначаются в этом же порядке начиная с 1.
pub fn resolve_fields(
    map: Option<&ProtobufSchemaFieldMap>,
    values: &HashMap<String, String>,
) -> Vec<(u64, PbType, String)> {
    let mut out = Vec::new();
    if let Some(m) = map {
        let mut names: Vec<&String> = m.fields.keys().collect();
        names.sort();
        for (i, name) in names.iter().enumerate() {
            let raw = &m.fields[*name];
            let (num, ty, tmpl) = parse_field_spec((i + 1) as u64, raw);
            out.push((num, ty, render_template(&tmpl, values)));
        }
    }
    out
}

/// Отрендерить схему в map name→value (для отладки/совместимости с прежним API).
pub fn apply_protobuf_schema(
    map: Option<&ProtobufSchemaFieldMap>,
    values: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(m) = map {
        for (k, v) in &m.fields {
            let (_, _, tmpl) = parse_field_spec(1, v);
            out.insert(k.clone(), render_template(&tmpl, values));
        }
    }
    out
}

/// Сериализовать сообщение в честный protobuf wire-format.
///
/// Если схема не задана, возвращается пустой буфер (валидное пустое
/// protobuf-сообщение).
pub fn serialize_protobuf(
    map: Option<&ProtobufSchemaFieldMap>,
    values: &HashMap<String, String>,
) -> Vec<u8> {
    let fields = resolve_fields(map, values);
    let mut buf = Vec::new();
    // Кодируем в порядке возрастания field_number (не обязательно по спеке,
    // но даёт детерминированный и канонический вывод).
    let mut sorted = fields;
    sorted.sort_by_key(|(num, _, _)| *num);
    for (num, ty, val) in &sorted {
        encode_field(&mut buf, *num, *ty, val);
    }
    buf
}

/// Обратно совместимое имя-обёртка (прежний вызов `serialize_protobuf_like`).
pub fn serialize_protobuf_like(
    map: Option<&ProtobufSchemaFieldMap>,
    values: &HashMap<String, String>,
) -> Vec<u8> {
    serialize_protobuf(map, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn varint_encoding_matches_spec() {
        let mut b = Vec::new();
        write_varint(&mut b, 1);
        assert_eq!(b, vec![0x01]);
        let mut b = Vec::new();
        write_varint(&mut b, 150);
        assert_eq!(b, vec![0x96, 0x01]); // канонический пример из спеки protobuf
        let mut b = Vec::new();
        write_varint(&mut b, 300);
        assert_eq!(b, vec![0xAC, 0x02]);
    }

    #[test]
    fn zigzag_matches_spec() {
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
        assert_eq!(zigzag(-2), 3);
        assert_eq!(zigzag(2147483647), 4294967294);
    }

    #[test]
    fn string_field_wire_format() {
        // field 1, wire_type 2 (LEN), value "testing" — канонический пример из спеки.
        let mut buf = Vec::new();
        encode_field(&mut buf, 1, PbType::Str, "testing");
        // tag = (1<<3)|2 = 0x0A, len = 7, затем "testing"
        assert_eq!(buf[0], 0x0A);
        assert_eq!(buf[1], 0x07);
        assert_eq!(&buf[2..], b"testing");
    }

    #[test]
    fn int_field_wire_format() {
        // field 1, VARINT, value 150 → tag 0x08, затем 0x96 0x01
        let mut buf = Vec::new();
        encode_field(&mut buf, 1, PbType::Int, "150");
        assert_eq!(buf, vec![0x08, 0x96, 0x01]);
    }

    #[test]
    fn parse_spec_variants() {
        assert_eq!(
            parse_field_spec(1, "hello"),
            (1, PbType::Str, "hello".to_string())
        );
        assert_eq!(
            parse_field_spec(1, "3:hello"),
            (3, PbType::Str, "hello".to_string())
        );
        assert_eq!(
            parse_field_spec(1, "3:int:42"),
            (3, PbType::Int, "42".to_string())
        );
        // Двоеточие в шаблоне без номера/типа — весь текст остаётся шаблоном.
        assert_eq!(
            parse_field_spec(2, "time:now"),
            (2, PbType::Str, "time:now".to_string())
        );
    }

    #[test]
    fn full_message_is_decodable_order() {
        let mut m = ProtobufSchemaFieldMap::default();
        m.fields
            .insert("b_num".to_string(), "2:int:150".to_string());
        m.fields
            .insert("a_str".to_string(), "1:testing".to_string());
        let out = serialize_protobuf(Some(&m), &vals());
        // Ожидаем field 1 (string testing), затем field 2 (int 150).
        assert_eq!(
            out,
            vec![0x0A, 0x07, b't', b'e', b's', b't', b'i', b'n', b'g', 0x10, 0x96, 0x01]
        );
    }

    #[test]
    fn empty_schema_yields_empty_buffer() {
        assert!(serialize_protobuf(None, &vals()).is_empty());
    }

    #[test]
    fn bool_and_double_fields() {
        let mut buf = Vec::new();
        encode_field(&mut buf, 1, PbType::Bool, "true");
        assert_eq!(buf, vec![0x08, 0x01]);
        let mut buf = Vec::new();
        encode_field(&mut buf, 2, PbType::Double, "1.0");
        // tag = (2<<3)|1 = 0x11, затем 8 байт f64 LE для 1.0
        assert_eq!(buf[0], 0x11);
        assert_eq!(&buf[1..], &1.0f64.to_le_bytes());
    }
}
