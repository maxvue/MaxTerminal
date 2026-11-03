//! Tailing incremental e seguro de arquivos JSONL (`JsonlTail`) com detecção de truncamento/rotação (`CA-18`, `CA-34`).

use std::io::SeekFrom;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use serde_json::Value;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Leitor incremental de arquivos JSONL com proteção contra linhas gigantes, JSON malformado e truncamento.
pub struct JsonlTail {
    path: PathBuf,
    offset: u64,
    dev_ino: Option<(u64, u64)>,
    partial: Vec<u8>,
    skip_until_newline: bool,
}

/// Resultado de uma leitura incremental em `JsonlTail::read_new`.
#[derive(Debug, Default)]
pub struct TailReadBatch {
    /// Registros JSON válidos lidos neste lote.
    pub records: Vec<Value>,
    /// `true` quando o arquivo foi truncado ou substituído (mudança de `inode`), exigindo invalidação do estado anterior.
    pub reset_detected: bool,
}

impl JsonlTail {
    /// Janela inicial máxima lida ao abrir um arquivo existente (256 KiB).
    pub const INITIAL_WINDOW: u64 = 256 * 1024;
    /// Tamanho máximo permitido para uma única linha JSONL (64 KiB).
    pub const MAX_LINE_BYTES: usize = 64 * 1024;

    /// Abre um arquivo JSONL posicionando nos últimos `INITIAL_WINDOW` bytes (descartando a primeira linha parcial).
    pub async fn open_from_end(path: PathBuf) -> std::io::Result<Self> {
        let meta = tokio::fs::symlink_metadata(&path).await?;
        if !meta.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "O caminho do transcript JSONL não é um arquivo regular.",
            ));
        }

        let current_uid = nix::unistd::geteuid().as_raw();
        if meta.uid() != current_uid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "O arquivo JSONL pertence a outro UID.",
            ));
        }

        let len = meta.len();
        let (offset, skip_until_newline) = if len > Self::INITIAL_WINDOW {
            (len - Self::INITIAL_WINDOW, true)
        } else {
            (0, false)
        };

        Ok(Self {
            path,
            offset,
            dev_ino: Some((meta.dev(), meta.ino())),
            partial: Vec::new(),
            skip_until_newline,
        })
    }

    /// Lê os novos bytes adicionados ao arquivo desde a última leitura.
    pub async fn read_new(&mut self) -> std::io::Result<TailReadBatch> {
        let meta = tokio::fs::symlink_metadata(&self.path).await?;
        if !meta.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "O caminho deixou de ser um arquivo regular.",
            ));
        }

        let current_uid = nix::unistd::geteuid().as_raw();
        if meta.uid() != current_uid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "UID do arquivo JSONL inválido.",
            ));
        }

        let current_dev_ino = (meta.dev(), meta.ino());
        let len = meta.len();
        let mut reset_detected = false;

        if self.dev_ino != Some(current_dev_ino) || len < self.offset {
            self.dev_ino = Some(current_dev_ino);
            self.partial.clear();
            self.offset = if len > Self::INITIAL_WINDOW {
                self.skip_until_newline = true;
                len - Self::INITIAL_WINDOW
            } else {
                self.skip_until_newline = false;
                0
            };
            reset_detected = true;
        }

        if len == self.offset {
            return Ok(TailReadBatch {
                records: Vec::new(),
                reset_detected,
            });
        }

        let to_read = (len - self.offset).min(Self::INITIAL_WINDOW * 2);
        let mut file = File::open(&self.path).await?;
        file.seek(SeekFrom::Start(self.offset)).await?;

        let mut buf = Vec::with_capacity(usize::try_from(to_read).unwrap_or(65536));
        let n = file.take(to_read).read_to_end(&mut buf).await?;
        self.offset += n as u64;

        let mut records = Vec::new();
        for &byte in &buf[..n] {
            if self.skip_until_newline {
                if byte == b'\n' {
                    self.skip_until_newline = false;
                }
                continue;
            }

            if byte == b'\n' {
                if !self.partial.is_empty() && self.partial.len() <= Self::MAX_LINE_BYTES {
                    if let Ok(val) = serde_json::from_slice::<Value>(&self.partial) {
                        records.push(val);
                    }
                }
                self.partial.clear();
            } else if self.partial.len() <= Self::MAX_LINE_BYTES {
                self.partial.push(byte);
            }
        }

        Ok(TailReadBatch {
            records,
            reset_detected,
        })
    }
}
