//! Subsistema de parsing incremental e decodificação de sequências `OSC 133`, `OSC 633` e `OSC 7`.

pub mod escape;
pub mod scanner;

pub use escape::{
    decode_base64_utf8, decode_file_uri_cwd, normalize_and_validate_cwd, unescape_osc_value,
};
pub use scanner::{
    DEFAULT_MAX_OSC_PAYLOAD, OscKind, OscScanner, RawOscEvent, ScanOutput, ScanSegment, TextSpan,
};
