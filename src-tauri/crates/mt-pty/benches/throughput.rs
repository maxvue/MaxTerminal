//! Benchmark `criterion` de vazão do pipeline binário de I/O (`mt-pty::io`).
//!
//! Avalia a taxa de transferência em MiB/s e frames/s combinando `ReplayRing`, `FlowControl`,
//! `encode_data_frame` e `SharedOutputState` sob `NullSink`, `AckingSink` e `SlowSink`.
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use mt_core::ids::PtyId;
use mt_pty::io::{
    EventFdWake, FlowControl, MonotonicClock, ReplayRing, SharedOutputState, decode_frame,
    encode_data_frame,
};
use mt_pty::sink::{NullSink, OutputSink, SinkClosed};
use mt_pty::tap::TapRegistry;

struct AckingSink {
    bytes_received: std::sync::atomic::AtomicU64,
}

impl AckingSink {
    const fn new() -> Self {
        Self {
            bytes_received: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl OutputSink for AckingSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        if let Ok(decoded) = decode_frame(frame) {
            let len = u64::try_from(decoded.payload.len()).unwrap_or(0);
            self.bytes_received
                .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }
}

struct SlowSink {
    bytes_received: std::sync::atomic::AtomicU64,
}

impl SlowSink {
    const fn new() -> Self {
        Self {
            bytes_received: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl OutputSink for SlowSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        if let Ok(decoded) = decode_frame(frame) {
            let len = u64::try_from(decoded.payload.len()).unwrap_or(0);
            self.bytes_received
                .fetch_add(len, std::sync::atomic::Ordering::Relaxed);
        }
        std::hint::spin_loop();
        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("pty_io_pipeline");
    let chunk_size = 64 * 1024; // 64 KiB por frame
    let chunks_per_iter = 64usize; // 4 MiB por iteração
    let total_bytes = u64::try_from(chunk_size * chunks_per_iter).unwrap_or(0);
    group.throughput(Throughput::Bytes(total_bytes));

    let payload = Bytes::from(vec![b'X'; chunk_size]);

    group.bench_with_input(
        BenchmarkId::new("encode_and_replay_ring", "64KiB_x_64"),
        &payload,
        |b, chunk| {
            b.iter(|| {
                let mut ring = ReplayRing::new(1024 * 1024);
                for _ in 0..chunks_per_iter {
                    let stored = ring.push_frame(chunk.clone());
                    let frame =
                        encode_data_frame(stored.seq, stored.byte_offset, &stored.payload).ok();
                    black_box(frame);
                }
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("shared_output_null_sink", "64KiB_x_64"),
        &payload,
        |b, chunk| {
            let pty_id = PtyId::new();
            let taps = Arc::new(TapRegistry::new());
            let Ok(wake_inner) = EventFdWake::new() else {
                return;
            };
            let wake = Arc::new(wake_inner);
            let flow = Arc::new(FlowControl::with_wake(
                1024 * 1024,
                256 * 1024,
                Duration::from_secs(5),
                Arc::new(MonotonicClock),
                Some(Arc::clone(&wake)),
            ));
            let state = SharedOutputState::new(1024 * 1024, flow, taps, wake);
            let Ok(info) = state.attach(Arc::new(NullSink), None) else {
                return;
            };
            let mut acked = 0u64;

            b.iter(|| {
                for _ in 0..chunks_per_iter {
                    state.publish_live_data(pty_id, chunk.clone());
                    acked = acked.saturating_add(u64::try_from(chunk.len()).unwrap_or(0));
                    let _ = state.ack(info.attachment_id, acked);
                }
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("shared_output_acking_sink", "64KiB_x_64"),
        &payload,
        |b, chunk| {
            let pty_id = PtyId::new();
            let taps = Arc::new(TapRegistry::new());
            let Ok(wake_inner) = EventFdWake::new() else {
                return;
            };
            let wake = Arc::new(wake_inner);
            let flow = Arc::new(FlowControl::with_wake(
                1024 * 1024,
                256 * 1024,
                Duration::from_secs(5),
                Arc::new(MonotonicClock),
                Some(Arc::clone(&wake)),
            ));
            let state = SharedOutputState::new(1024 * 1024, flow, taps, wake);
            let sink = Arc::new(AckingSink::new());
            let Ok(info) = state.attach(sink.clone(), None) else {
                return;
            };

            b.iter(|| {
                for _ in 0..chunks_per_iter {
                    state.publish_live_data(pty_id, chunk.clone());
                    let consumed = sink
                        .bytes_received
                        .load(std::sync::atomic::Ordering::Relaxed);
                    let _ = state.ack(info.attachment_id, consumed);
                }
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("shared_output_slow_sink", "64KiB_x_64"),
        &payload,
        |b, chunk| {
            let pty_id = PtyId::new();
            let taps = Arc::new(TapRegistry::new());
            let Ok(wake_inner) = EventFdWake::new() else {
                return;
            };
            let wake = Arc::new(wake_inner);
            let flow = Arc::new(FlowControl::with_wake(
                1024 * 1024,
                256 * 1024,
                Duration::from_secs(5),
                Arc::new(MonotonicClock),
                Some(Arc::clone(&wake)),
            ));
            let state = SharedOutputState::new(1024 * 1024, flow, taps, wake);
            let sink = Arc::new(SlowSink::new());
            let Ok(info) = state.attach(sink.clone(), None) else {
                return;
            };

            b.iter(|| {
                for _ in 0..chunks_per_iter {
                    state.publish_live_data(pty_id, chunk.clone());
                    let consumed = sink
                        .bytes_received
                        .load(std::sync::atomic::Ordering::Relaxed);
                    let _ = state.ack(info.attachment_id, consumed);
                }
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_throughput);
criterion_main!(benches);
