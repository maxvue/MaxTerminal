//! Benchmark `criterion` de vazão do sanitizador de segredos (`mt-db::sanitizer`).

#![allow(missing_docs, clippy::unwrap_used)]

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mt_db::sanitizer::{StreamSanitizer, sanitize};

fn build_clean_corpus(target_bytes: usize) -> String {
    let mut out = String::with_capacity(target_bytes + 256);
    let mut line_no = 0usize;
    while out.len() < target_bytes {
        line_no += 1;
        let _ = writeln!(
            out,
            "[{line_no:06}] INFO mt_pty::supervisor: compilando módulo src/components/TerminalPane.vue em http://localhost:5177 (hash e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855)"
        );
    }
    out.truncate(target_bytes);
    out
}

fn build_mixed_corpus(target_bytes: usize) -> String {
    let mut out = String::with_capacity(target_bytes + 512);
    let mut block = 0usize;
    while out.len() < target_bytes {
        block += 1;
        for i in 0..24 {
            let _ = writeln!(
                out,
                "[{block:04}:{i:02}] DEBUG cargo test --workspace -- running test_case_{i} ok"
            );
        }
        let _ = writeln!(
            out,
            "export GITHUB_TOKEN=ghp_1234567890abcdefghijklmnopqrstuvwxyzABCD OPENAI_API_KEY=sk-proj-1234567890abcdefghijklmnopqrstuvwxyzABCD"
        );
    }
    out.truncate(target_bytes);
    out
}

fn bench_sanitizer(c: &mut Criterion) {
    let size = 1024 * 1024; // 1 MiB
    let clean_corpus = build_clean_corpus(size);
    let mixed_corpus = build_mixed_corpus(size);

    let mut group = c.benchmark_group("mt_db_sanitizer");
    group.throughput(Throughput::Bytes(size as u64));

    group.bench_function("sanitize_clean_1mib", |b| {
        b.iter(|| {
            let res = sanitize(black_box(&clean_corpus));
            black_box(res.len());
        });
    });

    group.bench_function("sanitize_mixed_1mib", |b| {
        b.iter(|| {
            let res = sanitize(black_box(&mixed_corpus));
            black_box(res.len());
        });
    });

    group.bench_function("stream_sanitizer_mixed_64k_chunks", |b| {
        b.iter(|| {
            let mut stream = StreamSanitizer::new();
            let mut total_bytes = 0usize;
            for chunk in mixed_corpus.as_bytes().chunks(64 * 1024) {
                total_bytes += stream.push(black_box(chunk)).len();
            }
            total_bytes += stream.finish().len();
            black_box(total_bytes);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_sanitizer);
criterion_main!(benches);
