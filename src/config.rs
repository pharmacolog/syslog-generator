use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    #[serde(default)]
    pub templates: Vec<String>,
    pub templates_file: Option<String>,
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
