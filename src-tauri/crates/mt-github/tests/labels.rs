//! Testes do catálogo de 24 labels canônicas do `MaxCode` e mutação atômica (`mt-github::labels`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use mt_core::ids::ProjectId;
use mt_github::labels::{
    canonical_labels_catalog, compute_next_issue_labels, find_canonical_label,
    find_missing_canonical_labels, to_repo_label,
};
use mt_github::model::{CanonicalLabelGroup, ExclusiveLabelChange, LabelUpdatePatch};

#[test]
fn catalog_contains_exact_24_canonical_maxcode_labels_with_15_3_6_split() {
    let catalog = canonical_labels_catalog();
    assert_eq!(catalog.len(), 24);

    let state_count = catalog
        .iter()
        .filter(|l| l.group == CanonicalLabelGroup::State)
        .count();
    let closure_count = catalog
        .iter()
        .filter(|l| l.group == CanonicalLabelGroup::Closure)
        .count();
    let category_count = catalog
        .iter()
        .filter(|l| l.group == CanonicalLabelGroup::Category)
        .count();

    assert_eq!(state_count, 15);
    assert_eq!(closure_count, 3);
    assert_eq!(category_count, 6);

    assert!(find_canonical_label("waiting plan").is_some());
    assert!(find_canonical_label("Waiting user test").is_some());
    assert!(find_canonical_label("Evidence not Found").is_some());
    assert!(find_canonical_label("Regra de Negocio").is_some());
}

#[test]
fn atomic_label_update_enforces_state_and_closure_exclusivity_preserving_categories_and_custom() {
    let current = vec![
        String::from("Waiting Plan"),
        String::from("Regra de Negocio"),
        String::from("priority:high"),
    ];

    let patch = LabelUpdatePatch {
        project_id: ProjectId::new(),
        issue_number: 42,
        state_change: ExclusiveLabelChange::Set(String::from("Planned")),
        closure_change: ExclusiveLabelChange::Keep,
        add_categories: vec![String::from("Testes")],
        remove_categories: Vec::new(),
        add_custom: vec![String::from("frontend")],
        remove_custom: Vec::new(),
        client_mutation_id: None,
    };

    let updated = compute_next_issue_labels(&current, &patch).unwrap();
    assert_eq!(
        updated,
        vec![
            String::from("Planned"),
            String::from("Regra de Negocio"),
            String::from("Testes"),
            String::from("priority:high"),
            String::from("frontend"),
        ]
    );

    // Aplicando encerramento e limpando estado
    let patch_close = LabelUpdatePatch {
        project_id: ProjectId::new(),
        issue_number: 42,
        state_change: ExclusiveLabelChange::Clear,
        closure_change: ExclusiveLabelChange::Set(String::from("Duplicated")),
        add_categories: Vec::new(),
        remove_categories: vec![String::from("Regra de Negocio")],
        add_custom: Vec::new(),
        remove_custom: vec![String::from("priority:high")],
        client_mutation_id: None,
    };

    let closed_labels = compute_next_issue_labels(&updated, &patch_close).unwrap();
    assert_eq!(
        closed_labels,
        vec![
            String::from("Duplicated"),
            String::from("Testes"),
            String::from("frontend"),
        ]
    );
}

#[test]
fn detects_missing_canonical_labels_case_insensitively() {
    let existing = vec![
        to_repo_label("waiting plan", Some("0E8A16"), None),
        to_repo_label("Planned", Some("1D76DB"), None),
        to_repo_label("custom-label", Some("123456"), None),
    ];

    let missing = find_missing_canonical_labels(&existing);
    assert_eq!(missing.len(), 22);
    assert!(!missing.iter().any(|m| m.name == "Waiting Plan"));
    assert!(!missing.iter().any(|m| m.name == "Planned"));
}
