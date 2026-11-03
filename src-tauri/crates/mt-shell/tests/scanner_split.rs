//! Testes de fragmentação arbitrária (2 e 3 chunks, byte a byte e `proptest` >= 2.000 casos)
//! para o `OscScanner` incremental (Fase 05).

#![allow(clippy::unwrap_used, clippy::similar_names)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use mt_shell::{OscKind, OscScanner, RawOscEvent};
use proptest::prelude::*;

fn build_rich_corpus() -> Vec<u8> {
    let cmd1 = STANDARD.encode("echo 'Olá Mundo 🚀 œ €' && ls -la");
    let cwd1 = STANDARD.encode("/home/user/projeto-ação");
    let cmd2 = STANDARD.encode("false");
    let cwd2 = STANDARD.encode("/tmp");

    let mut buf = Vec::new();
    buf.extend_from_slice(b"banner inicial \x1b[32mMaxTerminal\x1b[0m\r\n");
    // OSC desconhecido (OSC 0 título) deve ser preservado como texto
    buf.extend_from_slice(b"\x1b]0;Titulo da Janela\x07");
    // DCS ignorado contendo falso prefixo OSC interno
    buf.extend_from_slice(b"\x1bP1$r\x1b]133;A;nonce=falso\x07\x1b\\");
    // Ciclo 1 terminado com BEL (\x07) e ST (\x1b\\)
    buf.extend_from_slice(
        format!(
            "\x1b]133;P;cwd_b64={cwd1};shell=bash\x07\x1b]133;A;cl=m;aid=1;nonce=n123\x07\x1b]133;B;nonce=n123\x1b\\$ \x1b]133;C;nonce=n123;cmd_b64={cmd1};cwd_b64={cwd1}\x07Olá Mundo 🚀 œ €\r\n\x1b]133;D;0;nonce=n123;cwd_b64={cwd1}\x1b\\"
        )
        .as_bytes(),
    );
    // Ciclo 2 terminado com C1 ST (0x9C) e OSC 7
    buf.extend_from_slice(b"\x1b]7;file://localhost/tmp/novo%20dir\x07");
    buf.extend_from_slice(
        format!("\x1b]633;E;echo\\x20inline\\x3bok;n=n123\x07\x1b]133;C;nonce=n123;cmd_b64={cmd2};cwd_b64={cwd2}")
            .as_bytes(),
    );
    buf.push(0x9c);
    buf.extend_from_slice(
        format!("falha\r\n\x1b]133;D;42;nonce=n123;cwd_b64={cwd2}\x07fim").as_bytes(),
    );
    buf
}

fn scan_in_chunks(corpus: &[u8], split_points: &[usize]) -> (Vec<RawOscEvent>, Vec<u8>) {
    let mut scanner = OscScanner::new();
    let mut all_events = Vec::new();
    let mut all_text = Vec::new();

    let mut prev = 0;
    let mut seq = 1u64;
    for &point in split_points {
        let p = point.clamp(prev, corpus.len());
        if p > prev {
            let out = scanner.scan_chunk(seq, &corpus[prev..p]);
            all_events.extend(out.events.iter().cloned());
            all_text.extend_from_slice(&out.concatenated_text());
            seq += 1;
            prev = p;
        }
    }
    if prev < corpus.len() {
        let out = scanner.scan_chunk(seq, &corpus[prev..]);
        all_events.extend(out.events.iter().cloned());
        all_text.extend_from_slice(&out.concatenated_text());
    }
    scanner.finish();
    (all_events, all_text)
}

fn normalize_event_positions(events: &[RawOscEvent]) -> Vec<(OscKind, u64, u64)> {
    events
        .iter()
        .map(|ev| {
            (
                ev.kind.clone(),
                ev.start_pos.byte_offset,
                ev.end_pos.byte_offset,
            )
        })
        .collect()
}

#[test]
fn exhaustive_two_and_three_chunk_splits_produce_identical_events_and_offsets() {
    let corpus = build_rich_corpus();
    let (baseline_events, baseline_text) = scan_in_chunks(&corpus, &[]);
    let baseline_norm = normalize_event_positions(&baseline_events);
    assert!(baseline_norm.len() >= 8);

    // Todas as divisões em 2 chunks
    for i in 1..corpus.len() {
        let (evs, txt) = scan_in_chunks(&corpus, &[i]);
        assert_eq!(
            normalize_event_positions(&evs),
            baseline_norm,
            "Divergência em split de 2 chunks no ponto {i}"
        );
        assert_eq!(txt, baseline_text, "Texto divergiu no split {i}");
    }

    // Divisões em 3 chunks com passo de 3 bytes (cobrindo fronteiras de ESC, BEL, ST e UTF-8)
    for i in (1..corpus.len()).step_by(3) {
        for j in (i + 1..corpus.len()).step_by(5) {
            let (evs, txt) = scan_in_chunks(&corpus, &[i, j]);
            assert_eq!(
                normalize_event_positions(&evs),
                baseline_norm,
                "Divergência em split de 3 chunks nos pontos ({i}, {j})"
            );
            assert_eq!(txt, baseline_text);
        }
    }

    // Divisão extrema byte a byte (1 byte por chunk)
    let every_byte: Vec<usize> = (1..corpus.len()).collect();
    let (byte_events, byte_text) = scan_in_chunks(&corpus, &every_byte);
    assert_eq!(normalize_event_positions(&byte_events), baseline_norm);
    assert_eq!(byte_text, baseline_text);
}

#[test]
fn handles_can_sub_and_incomplete_eof_gracefully() {
    let mut scanner = OscScanner::new();
    // CAN (\x18) aborta OSC em andamento; depois vem um OSC válido e um incompleto no EOF
    let input = b"abc\x1b]133;A;nonce=n1\x18def\x1b]133;B;nonce=n1\x07\x1b]133;C;incompleto";
    let out = scanner.scan_chunk(1, input);
    scanner.finish();

    assert_eq!(out.events.len(), 1);
    assert!(matches!(out.events[0].kind, OscKind::PromptEnd { .. }));
    assert_eq!(out.concatenated_text(), b"abcdef");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn proptest_scanner_never_panics_and_preserves_invariants_across_random_splits(
        raw in proptest::collection::vec(any::<u8>(), 0..512),
        split1 in 0usize..512,
        split2 in 0usize..512,
    ) {
        let mut s_single = OscScanner::with_max_payload_bytes(128);
        let out_single = s_single.scan_chunk(1, &raw);
        s_single.finish();

        let p1 = split1.min(raw.len());
        let p2 = split2.min(raw.len());
        let (a, b) = if p1 <= p2 { (p1, p2) } else { (p2, p1) };

        let mut s_split = OscScanner::with_max_payload_bytes(128);
        let o1 = s_split.scan_chunk(1, &raw[..a]);
        let o2 = s_split.scan_chunk(2, &raw[a..b]);
        let o3 = s_split.scan_chunk(3, &raw[b..]);
        s_split.finish();

        let mut combined_text = Vec::new();
        combined_text.extend(o1.concatenated_text());
        combined_text.extend(o2.concatenated_text());
        combined_text.extend(o3.concatenated_text());

        let mut combined_events = Vec::new();
        combined_events.extend(o1.events);
        combined_events.extend(o2.events);
        combined_events.extend(o3.events);

        prop_assert_eq!(
            normalize_event_positions(&combined_events),
            normalize_event_positions(&out_single.events)
        );
        prop_assert_eq!(combined_text, out_single.concatenated_text());
    }
}
