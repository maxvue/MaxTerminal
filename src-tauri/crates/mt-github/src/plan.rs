//! Extrator determinístico da seção `## PLANO DE IMPLEMENTAÇÃO` de descrições e comentários de issues.

use crate::model::{ExtractedPlan, PlanSource};

/// Candidato de texto (corpo da issue ou comentário) para extração de `## PLANO DE IMPLEMENTAÇÃO`.
#[derive(Debug, Clone)]
pub struct PlanCandidateInput<'a> {
    /// Fonte do texto (`Body` ou `Comment`).
    pub source: PlanSource,
    /// ID do comentário quando `source == PlanSource::Comment`.
    pub comment_id: Option<i64>,
    /// Login do autor.
    pub author_login: &'a str,
    /// Timestamp de criação em ms.
    pub created_at_ms: i64,
    /// Timestamp de atualização em ms.
    pub updated_at_ms: i64,
    /// Conteúdo Markdown já sanitizado.
    pub markdown: &'a str,
}

/// Remove acentos latinos comuns e converte para uppercase para comparação canônica do cabeçalho.
fn normalize_heading_title(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .trim_end_matches('#')
        .trim()
        .trim_end_matches(':')
        .trim();
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        let mapped = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ç' | 'Ç' => 'C',
            other => other.to_ascii_uppercase(),
        };
        out.push(mapped);
    }
    // Colapsa múltiplos espaços internos
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Verifica se uma linha fora de bloco de código é um heading H1/H2/H3 e retorna `(level, title)`.
fn parse_atx_heading(line: &str) -> Option<(usize, &str)> {
    // Linhas indentadas com 4+ espaços ou tab são blocos de código indentados
    if line.starts_with("    ") || line.starts_with('\t') {
        return None;
    }
    let trimmed_start = line.trim_start_matches(' ');
    // Ignora citações (`>`)
    if trimmed_start.starts_with('>') {
        return None;
    }
    let hashes = trimmed_start.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed_start[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some((hashes, rest.trim()))
}

/// Detecta delimitador de cerca de código (`` ``` `` ou `~~~`) fora de citações e indentação >= 4.
fn parse_fence_marker(line: &str) -> Option<(u8, usize)> {
    if line.starts_with("    ") || line.starts_with('\t') {
        return None;
    }
    let trimmed = line.trim_start_matches(' ');
    if trimmed.starts_with('>') {
        return None;
    }
    let first = trimmed.as_bytes().first().copied()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let count = trimmed.bytes().take_while(|&b| b == first).count();
    if count >= 3 {
        Some((first, count))
    } else {
        None
    }
}

/// Extrai a **última** seção `## PLANO DE IMPLEMENTAÇÃO` válida de um documento Markdown,
/// ignorando cabeçalhos dentro de cercas de código (`` ``` `` / `~~~`), código indentado e citações (`>`).
///
/// A seção se estende até o próximo heading de nível `<= 2` (`# ` ou `## `) fora de cercas ou até o fim do texto.
#[must_use]
pub fn extract_last_plan_section_from_markdown(markdown: &str) -> Option<String> {
    if markdown.trim().is_empty() {
        return None;
    }

    let lines: Vec<&str> = markdown.lines().collect();
    let mut active_fence: Option<(u8, usize)> = None;
    let mut sections: Vec<(usize, usize)> = Vec::new();
    let mut current_plan_start: Option<usize> = None;

    for (idx, &line) in lines.iter().enumerate() {
        if let Some((fence_char, fence_len)) = active_fence {
            if let Some((marker_char, marker_len)) = parse_fence_marker(line) {
                let after_ticks = line.trim_start_matches(' ')[marker_len..].trim();
                if marker_char == fence_char && marker_len >= fence_len && after_ticks.is_empty() {
                    active_fence = None;
                }
            }
            continue;
        }

        if let Some(fence) = parse_fence_marker(line) {
            active_fence = Some(fence);
            continue;
        }

        if let Some((level, title)) = parse_atx_heading(line) {
            if level <= 2 {
                if let Some(start_idx) = current_plan_start.take() {
                    sections.push((start_idx, idx));
                }
                if level == 2 && normalize_heading_title(title) == "PLANO DE IMPLEMENTACAO" {
                    current_plan_start = Some(idx);
                }
            }
        }
    }

    if let Some(start_idx) = current_plan_start.take() {
        sections.push((start_idx, lines.len()));
    }

    // Percorre de trás para frente procurando a última seção que possua conteúdo não vazio abaixo do heading
    for (start, end) in sections.into_iter().rev() {
        if end <= start + 1 {
            continue;
        }
        let body_lines = &lines[(start + 1)..end];
        let body_joined = body_lines.join("\n");
        if body_joined.trim().is_empty() {
            continue;
        }
        let full_section = lines[start..end].join("\n").trim().to_string();
        return Some(full_section);
    }

    None
}

/// Resolve o `ExtractedPlan` vencedor entre a descrição da issue (`body`) e seus comentários,
/// selecionando aquele com maior `(updated_at_ms, created_at_ms, comment_id.unwrap_or(0))`.
#[must_use]
pub fn resolve_issue_plan(
    body: &PlanCandidateInput<'_>,
    comments: &[PlanCandidateInput<'_>],
    comments_complete: bool,
) -> Option<ExtractedPlan> {
    let mut best: Option<((i64, i64, i64), PlanSource, Option<i64>, String, String)> = None;

    for candidate in std::iter::once(body).chain(comments.iter()) {
        if let Some(section_md) = extract_last_plan_section_from_markdown(candidate.markdown) {
            let key = (
                candidate.updated_at_ms,
                candidate.created_at_ms,
                candidate.comment_id.unwrap_or(0),
            );
            let should_replace = best
                .as_ref()
                .is_none_or(|(best_key, _, _, _, _)| key >= *best_key);
            if should_replace {
                best = Some((
                    key,
                    candidate.source,
                    candidate.comment_id,
                    candidate.author_login.to_string(),
                    section_md,
                ));
            }
        }
    }

    best.map(
        |((updated_at_ms, _, _), source, comment_id, author_login, markdown)| ExtractedPlan {
            source,
            comment_id,
            author_login,
            updated_at_ms,
            markdown,
            provisional: !comments_complete,
        },
    )
}
