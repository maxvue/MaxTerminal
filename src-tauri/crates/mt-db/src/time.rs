//! Utilitários determinísticos de tempo UNIX em milissegundos e formatação UTC.

use std::time::{SystemTime, UNIX_EPOCH};

/// Retorna o timestamp UNIX atual em milissegundos (`i64`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Formata um timestamp UNIX em milissegundos no padrão compacto UTC `YYYYMMDDTHHMMSSZ`.
#[must_use]
pub fn format_utc_compact(unix_ms: i64) -> String {
    let total_secs = unix_ms.div_euclid(1000);
    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);

    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Algoritmo civil de Howard Hinnant para conversão de dias desde 1970-01-01
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}{m:02}{d:02}T{hour:02}{minute:02}{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_compact_correctly() {
        assert_eq!(format_utc_compact(0), "19700101T000000Z");
        // 2026-04-17T15:30:45Z -> 1776439845000 ms
        assert_eq!(format_utc_compact(1_776_439_845_000), "20260417T153045Z");
    }
}
