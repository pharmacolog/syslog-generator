
use std::collections::HashMap;

pub fn render_template(template: &str, values: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in values {
        out = out.replace(&format!("{{{{{}}}}}", k), v);
    }
    out
}
