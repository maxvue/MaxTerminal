//! Benchmark Criterion para o `OscScanner` incremental (Fase 05).
//!
//! Valida throughput em streams puros sem `ESC` (fast-path `memchr`) e em corpus misto `ANSI + OSC 133`.

#![allow(missing_docs)]

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use mt_shell::OscScanner;

fn bench_osc_scanner(c: &mut Criterion) {
    let mut group = c.benchmark_group("osc_scanner");

    // 1. Stream de 1 MiB sem ESC (fast-path memchr)
    let plain_chunk: Vec<u8> = b"linha de log de compilacao sem sequencias de escape 1234567890\n"
        .iter()
        .copied()
        .cycle()
        .take(1024 * 1024)
        .collect();

    group.throughput(Throughput::Bytes(plain_chunk.len() as u64));
    group.bench_function("plain_no_esc_1mib", |b| {
        let mut scanner = OscScanner::new();
        let mut seq = 1u64;
        b.iter(|| {
            let out = scanner.scan_chunk(seq, black_box(&plain_chunk));
            black_box(out.text_spans.len());
            seq += 1;
        });
    });

    // 2. Corpus misto ANSI CSI + OSC 133 (~256 KiB)
    let sample = b"\x1b]133;A;cl=m;aid=1;nonce=0123456789abcdef\x07\x1b]133;B;nonce=0123456789abcdef\x07\x1b]133;C;nonce=0123456789abcdef;cmd_b64=ZWNobyBoZWxsbw==;cwd_b64=L3RtcA==\x07\x1b[32mCompiling\x1b[0m mt-shell v0.1.0\r\nhello world\r\n\x1b]133;D;0;nonce=0123456789abcdef;cwd_b64=L3RtcA==\x07";
    let mixed_chunk: Vec<u8> = sample.iter().copied().cycle().take(256 * 1024).collect();

    group.throughput(Throughput::Bytes(mixed_chunk.len() as u64));
    group.bench_function("mixed_ansi_osc133_256kib", |b| {
        let mut scanner = OscScanner::new();
        let mut seq = 1u64;
        b.iter(|| {
            let out = scanner.scan_chunk(seq, black_box(&mixed_chunk));
            black_box(out.events.len());
            seq += 1;
        });
    });

    group.finish();
}

criterion_group!(benches, bench_osc_scanner);
criterion_main!(benches);
