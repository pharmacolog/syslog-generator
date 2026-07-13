use crate::error::ConfigError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TargetConfig {
    pub address: String,
    pub transport: String,
    #[serde(default = "default_connections")]
    pub connections: usize,
    #[serde(default = "default_weight")]
    pub weight: usize,
    /// Фрейминг для потоковых транспортов (tcp/tls) по RFC 6587:
    /// "octet-counting" (MSG-LEN SP SYSLOG-MSG) или "non-transparent" (SYSLOG-MSG + LF).
    /// Для udp/file игнорируется (там каждое сообщение — отдельная единица/строка).
    #[serde(default = "default_framing")]
    pub framing: String,
    /// N4 (безопасный TLS): имя хоста для SNI и проверки имени в сертификате.
    /// None → берётся хост из `address` (часть до ':'). Игнорируется для не-TLS.
    #[serde(default)]
    pub tls_domain: Option<String>,
    /// N4: путь к PEM-файлу доверенного CA-сертификата (для self-signed/
    /// приватного CA). Добавляется к системным корням доверия. None → только системные.
    #[serde(default)]
    pub tls_ca_file: Option<String>,
    /// N4: явный opt-in в НЕБЕЗОПАСНЫЙ режим (принять любой сертификат,
    /// отключить проверку имени). По умолчанию false — сертификаты проверяются.
    #[serde(default)]
    pub tls_insecure: bool,
    /// N4.mTLS (v8.7.2): путь к клиентскому PEM-сертификату для mTLS.
    /// Если задан, TLS-handshake предъявляет этот сертификат серверу.
    /// Парный файл — `tls_client_key_file`. None → клиент не предъявляет
    /// сертификат (one-way TLS). Игнорируется для не-TLS.
    #[serde(default)]
    pub tls_client_cert_file: Option<String>,
    /// N4.mTLS: путь к клиентскому PEM-ключу. Должен соответствовать
    /// сертификату из `tls_client_cert_file`. PEM-формат PKCS#8.
    #[serde(default)]
    pub tls_client_key_file: Option<String>,
    /// N4.mTLS: минимальная допустимая версия TLS-протокола. Принимает
    /// "1.2" или "1.3" (по умолчанию — системная, обычно 1.0). Защита от
    /// downgrade-attack на устаревшие версии.
    #[serde(default)]
    pub tls_min_protocol_version: Option<String>,
}
/// Ручной `Default`, согласованный с serde-дефолтами: connections=1, weight=1,
/// framing="non-transparent". Это важно, чтобы `TargetConfig::default()` в коде
/// (и `..Default::default()`) проходил валидацию F13.
impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            address: String::new(),
            transport: String::new(),
            connections: default_connections(),
            weight: default_weight(),
            framing: default_framing(),
            tls_domain: None,
            tls_ca_file: None,
            tls_insecure: false,
            tls_client_cert_file: None,
            tls_client_key_file: None,
            tls_min_protocol_version: None,
        }
    }
}
fn default_connections() -> usize {
    1
}
fn default_weight() -> usize {
    1
}
fn default_framing() -> String {
    "non-transparent".to_string()
}

/// Параметры syslog-заголовка (RFC 5424 / RFC 3164). Все поля опциональные с
/// разумными умолчаниями. Строковые поля проходят подстановку шаблона ({{...}}),
/// поэтому в них можно использовать {{hostname}}, {{sequence}} и т.п.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SyslogConfig {
    /// Facility 0..23 (RFC 5424 §6.2.1). По умолчанию 1 (user-level).
    #[serde(default = "default_facility")]
    pub facility: u8,
    /// Severity 0..7 (RFC 5424 §6.2.1). По умолчанию 6 (informational).
    #[serde(default = "default_severity")]
    pub severity: u8,
    #[serde(default = "default_hostname")]
    pub hostname: String,
    #[serde(default = "default_app_name")]
    pub app_name: String,
    /// PROCID (RFC 5424). По умолчанию NILVALUE ("-").
    #[serde(default = "default_nil")]
    pub procid: String,
    /// MSGID (RFC 5424). По умолчанию NILVALUE ("-").
    #[serde(default = "default_nil")]
    pub msgid: String,
    /// STRUCTURED-DATA целиком как строка (напр. `[ex@32473 k="v"]`) или NILVALUE.
    #[serde(default = "default_nil")]
    pub structured_data: String,
    /// Добавлять ли UTF-8 BOM перед MSG (RFC 5424 §6.4). По умолчанию false.
    #[serde(default)]
    pub bom: bool,
}
impl Default for SyslogConfig {
    fn default() -> Self {
        Self {
            facility: default_facility(),
            severity: default_severity(),
            hostname: default_hostname(),
            app_name: default_app_name(),
            procid: default_nil(),
            msgid: default_nil(),
            structured_data: default_nil(),
            bom: false,
        }
    }
}
fn default_facility() -> u8 {
    1
}
fn default_severity() -> u8 {
    6
}
fn default_hostname() -> String {
    "{{hostname}}".to_string()
}
fn default_app_name() -> String {
    "syslog-generator".to_string()
}
fn default_nil() -> String {
    "-".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShutdownConfig {
    #[serde(default = "default_shutdown_mode")]
    pub mode: String,
    #[serde(default = "default_shutdown_drain_timeout_secs")]
    pub drain_timeout_secs: u64,
}
impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            mode: default_shutdown_mode(),
            drain_timeout_secs: default_shutdown_drain_timeout_secs(),
        }
    }
}
fn default_shutdown_mode() -> String {
    "drain".to_string()
}
fn default_shutdown_drain_timeout_secs() -> u64 {
    15
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProtobufSchemaFieldMap {
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Phase {
    pub name: String,
    /// Условие остановки по времени (сек). 0 = не ограничивать по времени.
    #[serde(default)]
    pub duration_secs: u64,
    /// Целевая интенсивность (сообщений в секунду). 0 = без ограничения скорости (max speed).
    #[serde(default)]
    pub messages_per_second: u64,
    /// Условие остановки по общему количеству сообщений. None = не ограничивать по количеству.
    #[serde(default)]
    pub total_messages: Option<u64>,
    /// F14: мультишаблоны — если задано и templates, и templates_file —
    /// приоритет у schema, иначе выбирается случайный из списка templates.
    /// `skip_serializing_if = "Vec::is_empty"` нужно для D3: пустой массив
    /// не сериализуется в JSON, и тогда anyOf `required: ["templates"]` в
    /// schemas/profile.schema.json корректно отлавливает фазы без
    /// контент-источника (только templates_file / schema_file).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<String>,
    /// `skip_serializing_if = "Option::is_none"` нужно для D3: None не
    /// сериализуется в JSON, и тогда anyOf `required: ["templates_file"]`
    /// в schema корректно отлавливает фазы без этого источника.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_file: Option<String>,
    pub format: Option<String>,
    pub output: Option<String>,
    pub seed: Option<u64>,
    pub protobuf_schema: Option<ProtobufSchemaFieldMap>,
    /// Параметры syslog-заголовка для rfc5424/rfc3164 (игнорируются для raw/protobuf).
    #[serde(default)]
    pub syslog: SyslogConfig,
    /// Профиль нагрузки во времени (F3): кривая интенсивности внутри фазы
    /// (constant/linear/sine/burst). None = постоянная интенсивность из
    /// `messages_per_second` (обратная совместимость).
    #[serde(default)]
    pub load_shape: Option<crate::load_shape::LoadShape>,
    /// F14: веса для выбора шаблона из `templates`/`templates_file`. Если задано
    /// и длина совпадает с числом шаблонов — выбор взвешенный; иначе равновероятный.
    #[serde(default)]
    pub template_weights: Option<Vec<f64>>,
    /// F6: паддинг тела сообщения до указанного размера в байтах (0/None = выкл).
    #[serde(default)]
    pub pad_to_bytes: Option<usize>,
    /// F15: конфигурация ArcSight CEF-формата (используется при `format: "cef"`).
    /// `None` — формат cef неприменим (валидатор F13 отвергнет такую фазу).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cef: Option<CefConfig>,
    /// F15: конфигурация IBM QRadar LEEF-формата (используется при `format: "leef"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leef: Option<LeefConfig>,
    /// F15: дополнительные поля верхнего уровня для JSON-lines формата
    /// (`{"ts":"...","level":"...","msg":"...","extra_field":"...",...}`).
    /// `None` — без доп. полей (только timestamp/level/msg).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_lines_fields: Option<std::collections::BTreeMap<String, String>>,
}

/// F15: конфигурация ArcSight Common Event Format (CEF).
///
/// CEF: `CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extension`
/// Severity 0..=10 (CEF-спецификация, не путать с syslog-severity 0..=7).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CefConfig {
    pub device_vendor: String,
    pub device_product: String,
    pub device_version: String,
    pub signature_id: String,
    pub name: String,
    /// CEF severity 0..=10. None = 0 (Unknown).
    #[serde(default)]
    pub severity: Option<u8>,
    /// Extension key=value pairs (CEF extension block). None = пустой extension.
    /// Значения подставляются через `render_template` (поддержка `{{faker.*}}`).
    #[serde(default)]
    pub extensions: Option<std::collections::BTreeMap<String, String>>,
}

/// F15: конфигурация IBM QRadar Log Event Extended Format (LEEF).
///
/// LEEF: `LEEF:Version|Vendor|Product|Version|EventID|...|key=value\n`
/// Attributes — key=value пары после разделителя (обычно TAB).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LeefConfig {
    pub vendor: String,
    pub product: String,
    pub version: String,
    pub event_id: String,
    /// LEEF attributes key=value pairs. None = без атрибутов (пустой хвост).
    /// Значения подставляются через `render_template`.
    #[serde(default)]
    pub attributes: Option<std::collections::BTreeMap<String, String>>,
}
impl Phase {
    pub fn format_type(&self) -> &str {
        self.format.as_deref().unwrap_or("rfc5424")
    }
}
fn default_distribution() -> String {
    "round-robin".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Profile {
    #[serde(default)]
    pub targets: Vec<TargetConfig>,
    #[serde(default = "default_distribution")]
    pub distribution: String,
    #[serde(default)]
    pub shutdown: ShutdownConfig,
    #[serde(default)]
    pub phases: Vec<Phase>,
    /// F12: адрес HTTP-эндпоинта /metrics (напр. "127.0.0.1:9090"). None →
    /// HTTP-сервер метрик не запускается.
    #[serde(default)]
    pub metrics_addr: Option<String>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            distribution: default_distribution(),
            shutdown: ShutdownConfig::default(),
            phases: Vec::new(),
            metrics_addr: None,
        }
    }
}

/// Загрузить профиль из JSON-строки.
///
/// Используется в основном из main.rs и тестов; внешние пользователи могут
/// вызывать [`load_profile_from_path`] для автоопределения формата.
pub fn load_profile_from_json_str(s: &str) -> Result<Profile, ConfigError> {
    serde_json::from_str(s).map_err(|source| ConfigError::Json {
        path: "<inline>".to_string(),
        source,
    })
}

/// Загрузить профиль из YAML-строки (D3, v8.5.0).
///
/// `serde_yaml` поддерживает те же serde-дерайвы, что и `serde_json`, поэтому
/// структура `Profile` парсится идентично. YAML чувствителен к отступам —
/// неявный фолбэк на JSON не делаем, чтобы ошибка была однозначной.
pub fn load_profile_from_yaml_str(s: &str) -> Result<Profile, ConfigError> {
    serde_yaml::from_str(s).map_err(|source| ConfigError::Yaml {
        path: "<inline>".to_string(),
        source,
    })
}

/// Загрузить профиль из файла с автоопределением формата по расширению.
///
/// - `.json`  → [`load_profile_from_json_str`]
/// - `.yaml` / `.yml` → [`load_profile_from_yaml_str`]
/// - любое другое расширение → `ConfigError::UnsupportedFormat`
///
/// Расширение проверяется **до** открытия файла — иначе пользователь с
/// опечаткой в имени (`profile.jsn`) сначала получает `Io(NotFound)`, и
/// только потом, посмотрев расширение, понимает что файл бы и не
/// распарсился. Это намеренная строгость.
pub fn load_profile_from_path(path: &Path) -> Result<Profile, ConfigError> {
    let path_str = path.to_string_lossy().into_owned();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    // Сначала проверяем расширение.
    match ext.as_deref() {
        Some("json") => {}
        Some("yaml") | Some("yml") => {}
        Some(other) => {
            return Err(ConfigError::UnsupportedFormat {
                path: path_str,
                extension: other.to_string(),
            });
        }
        None => {
            return Err(ConfigError::UnsupportedFormat {
                path: path_str,
                extension: String::new(),
            });
        }
    }
    // Только теперь читаем файл.
    let mut file = fs::File::open(path).map_err(|source| ConfigError::Io {
        path: path_str.clone(),
        source,
    })?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|source| ConfigError::Io {
            path: path_str.clone(),
            source,
        })?;
    match ext.as_deref() {
        Some("json") => serde_json::from_str(&buf).map_err(|source| ConfigError::Json {
            path: path_str,
            source,
        }),
        Some("yaml") | Some("yml") => {
            serde_yaml::from_str(&buf).map_err(|source| ConfigError::Yaml {
                path: path_str,
                source,
            })
        }
        _ => unreachable!("расширение уже проверено выше"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn minimal_yaml() -> &'static str {
        r#"
targets:
  - address: 127.0.0.1:514
    transport: tcp
distribution: round-robin
shutdown:
  mode: drain
  drain_timeout_secs: 15
phases:
  - name: yaml-test
    messages_per_second: 10
    total_messages: 5
    templates:
      - "yaml seq={{sequence}}"
"#
    }

    #[test]
    fn load_profile_from_yaml_str_parses_minimal() {
        let p = load_profile_from_yaml_str(minimal_yaml()).expect("yaml должен парситься");
        assert_eq!(p.distribution, "round-robin");
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].name, "yaml-test");
        assert_eq!(p.phases[0].total_messages, Some(5));
        assert_eq!(p.phases[0].templates, vec!["yaml seq={{sequence}}"]);
        assert_eq!(p.targets.len(), 1);
        assert_eq!(p.targets[0].address, "127.0.0.1:514");
        assert_eq!(p.targets[0].transport, "tcp");
    }

    #[test]
    fn load_profile_from_yaml_str_parses_load_shape_burst() {
        let yaml = r#"
targets:
  - address: 127.0.0.1:514
    transport: tcp
distribution: round-robin
phases:
  - name: burst
    duration_secs: 60
    templates: ["x"]
    load_shape:
      type: burst
      base_rate: 100
      burst_rate: 8000
      every_secs: 10
      burst_secs: 2
"#;
        let p = load_profile_from_yaml_str(yaml).expect("yaml с load_shape");
        let shape = p.phases[0]
            .load_shape
            .as_ref()
            .expect("load_shape должна распарситься");
        match shape {
            crate::load_shape::LoadShape::Burst {
                base_rate,
                burst_rate,
                every_secs,
                burst_secs,
            } => {
                assert_eq!(*base_rate, 100.0);
                assert_eq!(*burst_rate, 8000.0);
                assert_eq!(*every_secs, 10.0);
                assert_eq!(*burst_secs, 2.0);
            }
            other => panic!("ожидался LoadShape::Burst, got: {other:?}"),
        }
    }

    #[test]
    fn load_profile_from_yaml_str_reports_yaml_error_with_path() {
        // Битый YAML — нарушение структуры (несовпадение отступа).
        let bad = "targets:\n  - address: 127.0.0.1:514\n transport: tcp\n";
        let e = load_profile_from_yaml_str(bad).unwrap_err();
        match e {
            ConfigError::Yaml { path, .. } => assert_eq!(path, "<inline>"),
            other => panic!("ожидался ConfigError::Yaml, got: {other:?}"),
        }
    }

    #[test]
    fn load_profile_from_json_still_works() {
        let json = r#"{
            "distribution": "broadcast",
            "targets": [{"address": "/tmp/x.log", "transport": "file"}],
            "phases": [{"name": "j", "templates": ["x"]}]
        }"#;
        let p = load_profile_from_json_str(json).expect("json должен парситься");
        assert_eq!(p.distribution, "broadcast");
        assert_eq!(p.phases.len(), 1);
    }

    #[test]
    fn load_profile_from_path_dispatches_by_extension() {
        let dir = std::env::temp_dir();
        let json_path = dir.join("sg_cfg_test_dispatch.json");
        let yaml_path = dir.join("sg_cfg_test_dispatch.yaml");
        let yml_path = dir.join("sg_cfg_test_dispatch.yml");
        let unknown_path = dir.join("sg_cfg_test_dispatch.toml");

        let mut files = [
            (
                &json_path,
                r#"{"distribution":"round-robin","phases":[{"name":"j","templates":["x"]}]}"#,
            ),
            (&yaml_path, minimal_yaml()),
            (&yml_path, minimal_yaml()),
        ];
        for (path, content) in &mut files {
            let mut f = fs::File::create(path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }

        let p_json = load_profile_from_path(&json_path).expect("json path");
        assert_eq!(p_json.distribution, "round-robin");
        let p_yaml = load_profile_from_path(&yaml_path).expect("yaml path");
        assert_eq!(p_yaml.distribution, "round-robin");
        assert_eq!(p_yaml.phases[0].name, "yaml-test");
        let p_yml = load_profile_from_path(&yml_path).expect("yml path");
        assert_eq!(p_yml.phases[0].name, "yaml-test");

        let e = load_profile_from_path(&unknown_path).unwrap_err();
        match e {
            ConfigError::UnsupportedFormat { extension, .. } => {
                assert_eq!(extension, "toml");
            }
            other => panic!("ожидался UnsupportedFormat, got: {other:?}"),
        }

        // Cleanup.
        for (path, _) in &files {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_file(&unknown_path);
    }

    #[test]
    fn load_profile_from_path_io_error_for_missing_file() {
        let path = std::env::temp_dir().join("sg_cfg_test_definitely_missing_xyz.json");
        let e = load_profile_from_path(&path).unwrap_err();
        assert!(matches!(e, ConfigError::Io { .. }), "got: {e:?}");
    }
}
