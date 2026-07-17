//! Шаблонизация сообщений: парсинг `{{placeholder}}` и подстановка значений.
//!
//! N5 (v8.6.1): `CompiledTemplate` парсит шаблон один раз в вектор частей
//! (`Literal` / `Placeholder`), а потом рендерит за один проход по частям.
//! Это устраняет O(N×M) сложность старой реализации (где каждая
//! `String::replace` сканировала весь шаблон для каждого из M ключей).
//!
//! Старая функция `render_template(template, values)` сохранена как
//! backward-compatible обёртка — она компилирует шаблон на лету и
//! рендерит. Для горячего пути (например, send loop в core.rs) лучше
//! компилировать шаблон один раз и переиспользовать `CompiledTemplate`.

use std::collections::HashMap;

/// Часть распарсенного шаблона.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplatePart {
    /// Литеральный кусок текста (без плейсхолдеров внутри).
    Literal(String),
    /// Имя плейсхолдера (без обрамляющих `{{` `}}`).
    Placeholder(String),
}

/// Скомпилированный шаблон: результат парсинга + готов к быстрому рендерингу.
///
/// Парсинг (`CompiledTemplate::compile`) делается один раз. Рендеринг
/// (`render`) — линеен по длине шаблона: для каждой `Literal` части
/// копируется строка, для каждой `Placeholder` делается один `HashMap::get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledTemplate {
    parts: Vec<TemplatePart>,
}

impl CompiledTemplate {
    /// Распарсить шаблон в `CompiledTemplate`. Шаблон может содержать
    /// плейсхолдеры вида `{{key}}`. Невалидный `{{` без закрывающего `}}`
    /// остаётся как литерал (для обратной совместимости с `render_template`).
    pub fn compile(template: &str) -> Self {
        let mut parts = Vec::new();
        let mut current_literal = String::new();
        let mut chars = template.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            if c == '{' && chars.peek().copied().map(|x| x.1) == Some('{') {
                // Открывающий `{{`. Сохраняем накопленный литерал (если есть).
                if !current_literal.is_empty() {
                    parts.push(TemplatePart::Literal(std::mem::take(&mut current_literal)));
                }
                // Пропускаем второй `{`.
                chars.next();
                // Собираем имя плейсхолдера до `}}`.
                let mut name = String::new();
                let mut closed = false;
                while let Some((_, nc)) = chars.next() {
                    if nc == '}' && chars.peek().copied().map(|x| x.1) == Some('}') {
                        chars.next(); // пропускаем второй `}`
                        closed = true;
                        break;
                    }
                    name.push(nc);
                }
                if closed && !name.is_empty() {
                    parts.push(TemplatePart::Placeholder(name));
                } else {
                    // Невалидный `{{` без `}}` или пустое имя → возвращаем как литерал,
                    // сохраняя обратную совместимость со старым `render_template`:
                    // `String::replace("{{key", v)` тоже не находит `{{...}}` и
                    // возвращает шаблон as-is, поэтому наш литерал должен быть
                    // точно тем, что мы прочитали из шаблона (без добавления `}`).
                    current_literal.push_str("{{");
                    current_literal.push_str(&name);
                    // NB: недопарсенный `}` (если был) мы уже прочитали в name;
                    // закрывающие `}}` отсутствуют — оставляем как есть.
                }
            } else {
                current_literal.push(c);
            }
        }
        if !current_literal.is_empty() {
            parts.push(TemplatePart::Literal(current_literal));
        }
        CompiledTemplate { parts }
    }

    /// Подставить значения из `values` в шаблон. Один проход по частям,
    /// O(N) где N — длина шаблона (а не O(N×M) как у старого `String::replace`).
    /// Неизвестные плейсхолдеры оставляются как `{{name}}` (для обратной совместимости).
    ///
    /// PR-17a (v10.7.16): `#[inline(always)]` — hot-path, вызывается per msg
    /// в `core.rs:204,283`.
    #[inline(always)]
    pub fn render(&self, values: &HashMap<String, String>) -> String {
        // Преаллоцируем: длина результата ≈ длина шаблона + сумма длин значений.
        let mut out = String::with_capacity(
            self.parts
                .iter()
                .map(|p| match p {
                    TemplatePart::Literal(s) => s.len(),
                    TemplatePart::Placeholder(k) => {
                        values.get(k).map_or(2 + k.len() + 2, |v| v.len())
                    }
                })
                .sum(),
        );
        for part in &self.parts {
            match part {
                TemplatePart::Literal(s) => out.push_str(s),
                TemplatePart::Placeholder(k) => match values.get(k) {
                    Some(v) => out.push_str(v),
                    None => {
                        // Неизвестный плейсхолдер — оставляем как `{{name}}`
                        // для обратной совместимости со старым `render_template`.
                        out.push_str("{{");
                        out.push_str(k);
                        out.push_str("}}");
                    }
                },
            }
        }
        out
    }
}

/// Backward-compatible обёртка: парсит шаблон и сразу рендерит.
/// Для горячего пути предпочтительно `CompiledTemplate::compile()` +
/// многократный `render()`.
pub fn render_template(template: &str, values: &HashMap<String, String>) -> String {
    CompiledTemplate::compile(template).render(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_simple_template() {
        let ct = CompiledTemplate::compile("hello {{name}}");
        assert_eq!(
            ct.parts,
            vec![
                TemplatePart::Literal("hello ".to_string()),
                TemplatePart::Placeholder("name".to_string()),
            ]
        );
    }

    #[test]
    fn renders_literal_only() {
        let ct = CompiledTemplate::compile("no placeholders here");
        let mut v = HashMap::new();
        v.insert("x".to_string(), "y".to_string());
        assert_eq!(ct.render(&v), "no placeholders here");
    }

    #[test]
    fn renders_substituted_values() {
        let mut v = HashMap::new();
        v.insert("name".to_string(), "alice".to_string());
        v.insert("id".to_string(), "42".to_string());
        let ct = CompiledTemplate::compile("user={{name}} id={{id}}");
        assert_eq!(ct.render(&v), "user=alice id=42");
    }

    #[test]
    fn unknown_placeholder_preserved() {
        // Старое поведение: неизвестный `{{key}}` остаётся литералом `{{key}}`.
        let ct = CompiledTemplate::compile("{{known}} {{unknown}}");
        let mut v = HashMap::new();
        v.insert("known".to_string(), "yes".to_string());
        assert_eq!(ct.render(&v), "yes {{unknown}}");
    }

    #[test]
    fn empty_template() {
        let ct = CompiledTemplate::compile("");
        assert!(ct.parts.is_empty());
        let v = HashMap::new();
        assert_eq!(ct.render(&v), "");
    }

    #[test]
    fn malformed_placeholder_left_as_literal() {
        // Невалидный `{{key` без закрывающих `}}` остаётся литералом
        // (обратная совместимость со старым `render_template`).
        let ct = CompiledTemplate::compile("before {{unterminated and more");
        let v = HashMap::new();
        assert_eq!(ct.render(&v), "before {{unterminated and more");
    }

    #[test]
    fn back_compat_render_template_matches_old_behavior() {
        let mut v = HashMap::new();
        v.insert("x".to_string(), "1".to_string());
        v.insert("y".to_string(), "2".to_string());
        // Шаблон с разным порядком ключей в HashMap — render_template
        // даёт одинаковый результат в обеих реализациях.
        let template = "x={{x}} y={{y}} z={{z}}";
        let new = render_template(template, &v);
        assert_eq!(new, "x=1 y=2 z={{z}}");
    }

    #[test]
    fn performance_compile_once_render_many() {
        // Smoke-тест: убедиться что компиляция + многократный рендеринг не паникует.
        let template = "{{a}}-{{b}}-{{c}}-{{a}}";
        let ct = CompiledTemplate::compile(template);
        let mut v = HashMap::new();
        v.insert("a".to_string(), "X".to_string());
        v.insert("b".to_string(), "Y".to_string());
        v.insert("c".to_string(), "Z".to_string());
        for _ in 0..1000 {
            assert_eq!(ct.render(&v), "X-Y-Z-X");
        }
    }
}
