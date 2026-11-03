//! Produtor determinístico de rajadas finitas de PTY (`CA-16`) para os cenários:
//! - `--bytes <N>` (ex.: `1073741824` para 1 GiB contínuo numerado)
//! - `--file <PATH>` (ou geração e emissão de bloco de 500 MiB)
//!
//! Emite ao final em `stderr` um JSON auditável com `bytesProduced`, `linesProduced` e `sha256`.

#![allow(
    clippy::unwrap_used,
    clippy::large_stack_arrays,
    clippy::naive_bytecount,
    clippy::cast_possible_truncation
)]

use std::env;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut target_bytes: u64 = 1_048_576; // 1 MiB default em execução rápida
    let mut file_path: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bytes" if i + 1 < args.len() => {
                if let Ok(v) = args[i + 1].parse::<u64>() {
                    target_bytes = v;
                }
                i += 2;
            }
            "--file" if i + 1 < args.len() => {
                file_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let started = Instant::now();
    let stdout = io::stdout();
    let mut writer = BufWriter::with_capacity(64 * 1024, stdout.lock());
    let mut hasher = Sha256::new();
    let mut produced_bytes: u64 = 0;
    let mut produced_lines: u64 = 0;

    if let Some(path) = file_path {
        let f = File::open(&path).unwrap();
        let mut reader = BufReader::with_capacity(64 * 1024, f);
        let mut buf = [0_u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            let slice = &buf[..n];
            writer.write_all(slice).unwrap();
            hasher.update(slice);
            produced_bytes += n as u64;
            produced_lines += slice.iter().filter(|&&b| b == b'\n').count() as u64;
        }
    } else {
        let mut seq: u64 = 1;
        while produced_bytes < target_bytes {
            let line =
                format!("SEQ={seq:012} PAYLOAD=maxterminal_deterministic_burst_stream_frame_ok\n");
            let bytes = line.as_bytes();
            let remaining = (target_bytes - produced_bytes) as usize;
            let to_write = if bytes.len() <= remaining {
                bytes
            } else {
                &bytes[..remaining]
            };
            writer.write_all(to_write).unwrap();
            hasher.update(to_write);
            produced_bytes += to_write.len() as u64;
            produced_lines += 1;
            seq += 1;
        }
    }

    writer.flush().unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    let digest_hex = format!("{:x}", hasher.finalize());

    eprintln!(
        r#"{{"bytesProduced":{produced_bytes},"linesProduced":{produced_lines},"elapsedMs":{elapsed_ms},"sha256":"{digest_hex}"}}"#
    );
}
