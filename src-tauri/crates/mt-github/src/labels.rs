//! Catálogo das 24 labels canônicas do workflow `MaxCode` e motor de mutação atômica.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use serde::Deserialize;

use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::model::{
    CanonicalLabelGroup, CanonicalLabelSpec, ExclusiveLabelChange, LabelUpdatePatch, RepoLabel,
};

const LABELS_FIXTURE_JSON: &str = include_str!("../fixtures/maxcode/labels.v1.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureRoot {
    labels: Vec<CanonicalLabelSpec>,
}

static CANONICAL_LABELS: LazyLock<Vec<CanonicalLabelSpec>> = LazyLock::new(|| {
    let parsed: FixtureRoot = serde_json::from_str(LABELS_FIXTURE_JSON)
        .unwrap_or_else(|e| panic!("Fixture labels.v1.json inválida: {e}"));
    assert_eq!(
        parsed.labels.len(),
        24,
        "O catálogo canônico do MaxCode deve conter exatamente 24 labels"
    );
    parsed.labels
});

static BY_LOWER: LazyLock<HashMap<String, CanonicalLabelSpec>> = LazyLock::new(|| {
    CANONICAL_LABELS
        .iter()
        .map(|spec| (spec.name.to_ascii_lowercase(), spec.clone()))
        .collect()
});

/// Retorna a fatia estática das 24 labels canônicas do `MaxCode` na ordem canônica (`1..=24`).
#[must_use]
pub fn canonical_labels_catalog() -> &'static [CanonicalLabelSpec] {
    &CANONICAL_LABELS
}

/// Procura uma label canônica de forma case-insensitive.
#[must_use]
pub fn find_canonical_label(name: &str) -> Option<&'static CanonicalLabelSpec> {
    let key = name.trim().to_ascii_lowercase();
    BY_LOWER.get(&key)
}

/// Classifica o grupo canônico (`state`, `closure`, `category`) de um nome de label, se pertencer às 24.
#[must_use]
pub fn classify_label_group(name: &str) -> Option<CanonicalLabelGroup> {
    find_canonical_label(name).map(|spec| spec.group)
}

/// Retorna a grafia canônica se for uma das 24 labels ou o nome original higienizado caso seja customizada.
#[must_use]
pub fn normalize_label_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(spec) = find_canonical_label(trimmed) {
        spec.name.clone()
    } else {
        trimmed.to_string()
    }
}

/// Converte um nome de label em `RepoLabel` enriquecido com metadados canônicos quando disponível.
#[must_use]
pub fn to_repo_label(name: &str, color_hex: Option<&str>, description: Option<&str>) -> RepoLabel {
    if let Some(spec) = find_canonical_label(name) {
        RepoLabel {
            name: spec.name.clone(),
            name_lower: spec.name.to_ascii_lowercase(),
            color_hex: color_hex
                .map(|c| c.trim_start_matches('#').to_ascii_uppercase())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| spec.color_hex.clone()),
            description: description
                .filter(|d| !d.is_empty())
                .map_or_else(|| spec.description.clone(), ToString::to_string),
            canonical_group: Some(spec.group),
        }
    } else {
        let trimmed = name.trim();
        RepoLabel {
            name: trimmed.to_string(),
            name_lower: trimmed.to_ascii_lowercase(),
            color_hex: color_hex
                .map(|c| c.trim_start_matches('#').to_ascii_uppercase())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| String::from("64748B")),
            description: description.unwrap_or_default().to_string(),
            canonical_group: None,
        }
    }
}

/// Identifica quais das 24 labels canônicas estão ausentes na lista de labels atuais do repositório.
#[must_use]
pub fn find_missing_canonical_labels(
    existing_repo_labels: &[RepoLabel],
) -> Vec<CanonicalLabelSpec> {
    let present_lower: BTreeSet<String> = existing_repo_labels
        .iter()
        .map(|l| l.name.trim().to_ascii_lowercase())
        .collect();

    canonical_labels_catalog()
        .iter()
        .filter(|spec| !present_lower.contains(&spec.name.to_ascii_lowercase()))
        .cloned()
        .collect()
}

/// Extrai `(active_state_label, active_closure_label, category_labels)` de uma lista de labels.
#[must_use]
pub fn extract_canonical_facets(
    labels: &[RepoLabel],
) -> (Option<String>, Option<String>, Vec<String>) {
    let mut active_state: Option<(u8, String)> = None;
    let mut active_closure: Option<(u8, String)> = None;
    let mut categories: Vec<(u8, String)> = Vec::new();

    for label in labels {
        if let Some(spec) = find_canonical_label(&label.name) {
            match spec.group {
                CanonicalLabelGroup::State => {
                    if active_state
                        .as_ref()
                        .is_none_or(|(ord, _)| spec.order < *ord)
                    {
                        active_state = Some((spec.order, spec.name.clone()));
                    }
                }
                CanonicalLabelGroup::Closure => {
                    if active_closure
                        .as_ref()
                        .is_none_or(|(ord, _)| spec.order < *ord)
                    {
                        active_closure = Some((spec.order, spec.name.clone()));
                    }
                }
                CanonicalLabelGroup::Category => {
                    if !categories.iter().any(|(_, n)| n == &spec.name) {
                        categories.push((spec.order, spec.name.clone()));
                    }
                }
            }
        }
    }

    categories.sort_by_key(|(ord, _)| *ord);
    (
        active_state.map(|(_, n)| n),
        active_closure.map(|(_, n)| n),
        categories.into_iter().map(|(_, n)| n).collect(),
    )
}

/// Calcula o conjunto final de nomes de labels de uma issue após aplicar um `LabelUpdatePatch`,
/// garantindo exclusividade mútua em `state` (15) e em `closure` (3), preservando `category` (6)
/// e labels customizadas de terceiros.
pub fn compute_next_issue_labels(
    current_labels: &[String],
    patch: &LabelUpdatePatch,
) -> GithubResult<Vec<String>> {
    let mut state_selected: Option<String> = None;
    let mut closure_selected: Option<String> = None;
    let mut categories: BTreeSet<String> = BTreeSet::new();
    let mut custom_order: Vec<String> = Vec::new();
    let mut custom_lower_seen: BTreeSet<String> = BTreeSet::new();

    for raw in current_labels {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(spec) = find_canonical_label(trimmed) {
            match spec.group {
                CanonicalLabelGroup::State => {
                    if state_selected.is_none() {
                        state_selected = Some(spec.name.clone());
                    }
                }
                CanonicalLabelGroup::Closure => {
                    if closure_selected.is_none() {
                        closure_selected = Some(spec.name.clone());
                    }
                }
                CanonicalLabelGroup::Category => {
                    categories.insert(spec.name.clone());
                }
            }
        } else {
            let lower = trimmed.to_ascii_lowercase();
            if custom_lower_seen.insert(lower) {
                custom_order.push(trimmed.to_string());
            }
        }
    }

    // 1. Aplica mutação no grupo `state`
    match &patch.state_change {
        ExclusiveLabelChange::Keep => {}
        ExclusiveLabelChange::Clear => {
            state_selected = None;
        }
        ExclusiveLabelChange::Set(target) => {
            let spec = find_canonical_label(target).ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::ValidationFailed,
                    format!("A label '{target}' não pertence ao grupo de estado do MaxCode."),
                )
            })?;
            if spec.group != CanonicalLabelGroup::State {
                return Err(GithubError::new(
                    GithubErrorKind::ValidationFailed,
                    format!("A label '{}' não é uma label de estado.", spec.name),
                ));
            }
            state_selected = Some(spec.name.clone());
        }
    }

    // 2. Aplica mutação no grupo `closure`
    match &patch.closure_change {
        ExclusiveLabelChange::Keep => {}
        ExclusiveLabelChange::Clear => {
            closure_selected = None;
        }
        ExclusiveLabelChange::Set(target) => {
            let spec = find_canonical_label(target).ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::ValidationFailed,
                    format!("A label '{target}' não pertence ao grupo de encerramento do MaxCode."),
                )
            })?;
            if spec.group != CanonicalLabelGroup::Closure {
                return Err(GithubError::new(
                    GithubErrorKind::ValidationFailed,
                    format!("A label '{}' não é uma label de encerramento.", spec.name),
                ));
            }
            closure_selected = Some(spec.name.clone());
        }
    }

    // 3. Aplica adições/remoções de categorias
    for rem in &patch.remove_categories {
        if let Some(spec) = find_canonical_label(rem) {
            if spec.group == CanonicalLabelGroup::Category {
                categories.remove(&spec.name);
            }
        }
    }
    for add in &patch.add_categories {
        let spec = find_canonical_label(add).ok_or_else(|| {
            GithubError::new(
                GithubErrorKind::ValidationFailed,
                format!("A label '{add}' não pertence ao grupo de categoria do MaxCode."),
            )
        })?;
        if spec.group != CanonicalLabelGroup::Category {
            return Err(GithubError::new(
                GithubErrorKind::ValidationFailed,
                format!("A label '{}' não é uma label de categoria.", spec.name),
            ));
        }
        categories.insert(spec.name.clone());
    }

    // 4. Aplica remoções e adições de labels customizadas
    let remove_custom_lower: BTreeSet<String> = patch
        .remove_custom
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    custom_order.retain(|item| !remove_custom_lower.contains(&item.to_ascii_lowercase()));
    custom_lower_seen.retain(|lower| !remove_custom_lower.contains(lower));

    for add_raw in &patch.add_custom {
        let trimmed = add_raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(spec) = find_canonical_label(trimmed) {
            // Se alguém passou uma label canônica em add_custom, roteia para o grupo adequado
            match spec.group {
                CanonicalLabelGroup::State => state_selected = Some(spec.name.clone()),
                CanonicalLabelGroup::Closure => closure_selected = Some(spec.name.clone()),
                CanonicalLabelGroup::Category => {
                    categories.insert(spec.name.clone());
                }
            }
        } else {
            let lower = trimmed.to_ascii_lowercase();
            if custom_lower_seen.insert(lower) {
                custom_order.push(trimmed.to_string());
            }
        }
    }

    // 5. Reconstrói na ordem canônica (state -> closure -> categories em order 19..24 -> custom)
    let mut final_labels: Vec<String> = Vec::new();
    if let Some(st) = state_selected {
        final_labels.push(st);
    }
    if let Some(cl) = closure_selected {
        final_labels.push(cl);
    }
    for spec in canonical_labels_catalog() {
        if spec.group == CanonicalLabelGroup::Category && categories.contains(&spec.name) {
            final_labels.push(spec.name.clone());
        }
    }
    final_labels.extend(custom_order);

    Ok(final_labels)
}
