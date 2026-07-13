pub mod cli;
pub mod config;
pub mod core;
pub mod error;
pub mod load_shape;
pub mod metrics;
pub mod metrics_server;
pub mod payload;
#[cfg(test)]
mod payload_proptests;
pub mod protobuf;
pub mod schema;
pub mod schema_check;
pub mod sender;
pub mod shutdown;
pub mod syslog;
pub mod template;
pub mod validate;

pub use cli::{apply_overrides, parse_target, Args, Overrides};
pub use config::{
    load_profile_from_json_str, load_profile_from_path, load_profile_from_yaml_str, Phase, Profile,
    ProtobufSchemaFieldMap, ShutdownConfig, SyslogConfig, TargetConfig,
};
pub use core::{
    create_dispatcher, default_values, generate_message, load_schema, load_templates,
    run_phase_multi, run_profile,
};
pub use error::{ConfigError, DrainError, MetricsError, RuntimeError};
pub use load_shape::LoadShape;
pub use metrics::{create_metrics, gather_metrics, Metrics};
pub use metrics_server::{build_http_response, parse_request_line, route, serve as serve_metrics};
pub use payload::{
    derive_rng, faker, gen_from_regex, int_in_range, pad_to_size, random_string, weighted_index,
    zipf_index,
};
pub use protobuf::{apply_protobuf_schema, serialize_protobuf, serialize_protobuf_like, PbType};
pub use schema::{Schema, SchemaField};
pub use schema_check::{
    validate_against_embedded_schema, validate_against_schema, SchemaCheckError, PROFILE_SCHEMA,
};
pub use sender::{
    build_tls_connector, record_send, target_sender_file, Framing, SharedRx, TlsParams,
};
pub use shutdown::{graceful_drain_wait, shutdown_listener};
pub use syslog::{build_rfc3164, build_rfc5424, escape_sd_value, prival, Header};
pub use template::render_template;
pub use validate::{format_errors, validate_profile, ValidationError};
