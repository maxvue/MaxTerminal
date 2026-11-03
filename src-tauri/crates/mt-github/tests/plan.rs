//! Testes do extrator determinístico de `## PLANO DE IMPLEMENTAÇÃO` (`mt-github::plan`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use mt_github::model::PlanSource;
use mt_github::plan::{
    PlanCandidateInput, extract_last_plan_section_from_markdown, resolve_issue_plan,
};

#[test]
fn extracts_plan_ignoring_fenced_code_blocks_and_blockquotes() {
    let md = "\
# Contexto do Bug

Exemplo de template:
```markdown
## PLANO DE IMPLEMENTAÇÃO
Plano falso dentro de fence que deve ser ignorado!
```

> ## PLANO DE IMPLEMENTAÇÃO
> Plano dentro de citação também deve ser ignorado.

## PLANO DE IMPLEMENTAÇÃO
1. Atualizar `src/stores/githubIssues.ts`
2. Validar com `npm run check`

### Subtítulo preservado
Detalhes técnicos da correção.

## Observações Finais
Fora do plano.
";

    let extracted = extract_last_plan_section_from_markdown(md).unwrap();
    assert!(extracted.contains("1. Atualizar `src/stores/githubIssues.ts`"));
    assert!(extracted.contains("### Subtítulo preservado"));
    assert!(!extracted.contains("Plano falso dentro de fence"));
    assert!(!extracted.contains("Observações Finais"));
}

#[test]
fn selects_most_recent_plan_between_body_and_comments_and_marks_provisional() {
    let body = PlanCandidateInput {
        source: PlanSource::Body,
        comment_id: None,
        author_login: "johnattas",
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
        markdown: "## PLANO DE IMPLEMENTAÇÃO\nPlano v1 inicial no body.",
    };

    let comments = vec![
        PlanCandidateInput {
            source: PlanSource::Comment,
            comment_id: Some(101),
            author_login: "bot-maxcode",
            created_at_ms: 3_000,
            updated_at_ms: 3_500,
            markdown: "## Plano de Implementacao:\nPlano v2 revisado no comentário 101.",
        },
        PlanCandidateInput {
            source: PlanSource::Comment,
            comment_id: Some(102),
            author_login: "reviewer",
            created_at_ms: 4_000,
            updated_at_ms: 4_000,
            markdown: "Comentário comum sem seção de plano.",
        },
    ];

    let resolved = resolve_issue_plan(&body, &comments, false).unwrap();
    assert_eq!(resolved.source, PlanSource::Comment);
    assert_eq!(resolved.comment_id, Some(101));
    assert_eq!(resolved.author_login, "bot-maxcode");
    assert!(resolved.markdown.contains("Plano v2 revisado"));
    assert!(resolved.provisional);
}
