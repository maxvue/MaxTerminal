//! Testes de integração de `AiRepo` (preferências com revisão otimista, sanitização dupla,
//! cache de catálogo/diagnóstico e histórico com geração monotônica pós-limpeza).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use mt_ai::config::AiSelection;
use mt_ai::error::codes;
use mt_ai::ids::AiRequestId;
use mt_ai::repo::{
    AiHistoryFilter, AiHistoryItem, AiHistoryKind, AiRepo, CacheClearScope, CachedCatalogRow,
    CachedDiagnosisRow,
};
use mt_core::ids::ProjectId;
use mt_db::{Db, DbConfig};
use tempfile::TempDir;

#[tokio::test]
async fn settings_optimistic_concurrency_and_validation() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("ai.db"))).unwrap();
    let repo = AiRepo::new(db.clone());

    let initial = repo.get_settings().await.unwrap();
    assert_eq!(initial.revision, 1);
    assert!(initial.enabled);

    // URL fora de loopback sem allow_remote deve falhar com ai.url_not_loopback
    let mut bad_remote = initial.clone();
    bad_remote.base_url = Some("http://192.168.1.50:8787".to_string());
    let err = repo.save_settings(bad_remote).await.unwrap_err();
    assert_eq!(err.code, codes::AI_URL_NOT_LOOPBACK);

    // URL com credenciais embutidas deve falhar com ai.url_invalid
    let mut bad_creds = initial.clone();
    bad_creds.base_url = Some("http://admin:secret@127.0.0.1:8787".to_string());
    let err_creds = repo.save_settings(bad_creds).await.unwrap_err();
    assert_eq!(err_creds.code, codes::AI_URL_INVALID);

    // Atualização válida incrementa revision para 2
    let mut valid = initial.clone();
    valid.base_url = Some("http://127.0.0.1:8787".to_string());
    valid.default_selection = Some(AiSelection {
        harness_id: "claude-code".to_string(),
        model_id: "claude-sonnet-4".to_string(),
        reasoning_id: Some("medium".to_string()),
    });
    let saved = repo.save_settings(valid).await.unwrap();
    assert_eq!(saved.revision, 2);

    // Tentar salvar novamente com revision antiga (1) gera ai.settings_conflict
    let conflict = repo.save_settings(initial).await.unwrap_err();
    assert_eq!(conflict.code, codes::AI_SETTINGS_CONFLICT);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn history_sanitization_and_generation_prevents_repopulation() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("ai.db"))).unwrap();
    let repo = AiRepo::new(db.clone());

    let gen_before = repo.current_history_generation();
    let proj_id = ProjectId::new();
    let proj_id_str = proj_id.to_string();
    db.write(move |tx| {
        tx.execute(
            "INSERT INTO projects (id, title, icon, color, path, stack, identity_source, sort_order, missing, created_at, updated_at)
             VALUES (?1, 'MaxTerminal', 'lucide:terminal', '#00AAFF', '/tmp/proj', 'rust', 'auto', 0, 0, 1000, 1000);",
            rusqlite::params![proj_id_str],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "insert_test_proj"))?;
        Ok(())
    })
    .await
    .unwrap();

    let item = AiHistoryItem {
        id: AiRequestId::new(),
        kind: AiHistoryKind::Suggest,
        trigger: Some("at".to_string()),
        project_id: Some(proj_id),
        tab_id: None,
        pane_id: None,
        session_id: None,
        block_id: None,
        harness_id: Some("claude-code".to_string()),
        model_id: Some("sonnet".to_string()),
        reasoning_id: None,
        rag_slug: None,
        schema_id: None,
        prompt_sanitized: "export OPENAI_API_KEY=sk-proj-1234567890abcdefghijklmnop".to_string(),
        response_sanitized: Some(
            "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjMifQ.sig"
                .to_string(),
        ),
        risk_level: Some("safe".to_string()),
        status: "completed".to_string(),
        error_code: None,
        latency_ms: Some(120),
        created_at: "2026-11-01T12:00:00.000Z".to_string(),
    };

    assert!(repo.insert_history(gen_before, true, item).await.unwrap());

    let page = repo
        .list_history(&AiHistoryFilter {
            kind: Some(AiHistoryKind::Suggest),
            project_id: Some(proj_id),
            limit: 10,
            before_id: None,
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(
        !page.items[0]
            .prompt_sanitized
            .contains("sk-proj-1234567890")
    );
    assert!(
        !page.items[0]
            .response_sanitized
            .as_deref()
            .unwrap()
            .contains("eyJhbGciOi")
    );

    // Limpa histórico -> incrementa geração
    let removed = repo.clear_history(None).await.unwrap();
    assert_eq!(removed, 1);

    // Uma inserção atrasada com a geração antiga deve ser descartada (retorna false)
    let stale_item = AiHistoryItem {
        id: AiRequestId::new(),
        kind: AiHistoryKind::Suggest,
        trigger: Some("question".to_string()),
        project_id: Some(proj_id),
        tab_id: None,
        pane_id: None,
        session_id: None,
        block_id: None,
        harness_id: None,
        model_id: None,
        reasoning_id: None,
        rag_slug: None,
        schema_id: None,
        prompt_sanitized: "listar arquivos".to_string(),
        response_sanitized: Some("ls -la".to_string()),
        risk_level: Some("safe".to_string()),
        status: "completed".to_string(),
        error_code: None,
        latency_ms: Some(50),
        created_at: "2026-11-01T12:01:00.000Z".to_string(),
    };
    assert!(
        !repo
            .insert_history(gen_before, true, stale_item)
            .await
            .unwrap()
    );

    let after_clear = repo
        .list_history(&AiHistoryFilter {
            kind: None,
            project_id: None,
            limit: 10,
            before_id: None,
        })
        .await
        .unwrap();
    assert!(after_clear.items.is_empty());

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn diagnosis_and_catalog_cache_lifecycle() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("ai.db"))).unwrap();
    let repo = AiRepo::new(db.clone());

    let c_gen = repo.current_cache_generation();
    let cat_row = CachedCatalogRow {
        gateway_key: "gw-1".to_string(),
        payload_json: "[]".to_string(),
        etag: Some("\"v1\"".to_string()),
        fetched_at: "2026-11-01T10:00:00.000Z".to_string(),
    };
    assert!(repo.put_catalog_cache(c_gen, &cat_row).await.unwrap());
    let loaded_cat = repo.get_catalog_cache("gw-1").await.unwrap().unwrap();
    assert_eq!(loaded_cat.etag.as_deref(), Some("\"v1\""));

    let hash = "a".repeat(64);
    let diag_row = CachedDiagnosisRow {
        error_hash: hash.clone(),
        gateway_key: "gw-1".to_string(),
        project_id: None,
        summary: "Comando não encontrado".to_string(),
        fix_command: Some("npm install".to_string()),
        fix_risk: Some("safe".to_string()),
        harness_id: "claude-code".to_string(),
        model_id: "sonnet".to_string(),
        reasoning_id: None,
        prompt_version: 1,
        sanitizer_version: 1,
        hits: 0,
        created_at: "2026-11-01T10:00:00.000Z".to_string(),
        expires_at: "2026-12-01T10:00:00.000Z".to_string(),
    };
    assert!(repo.put_diagnosis_cache(c_gen, &diag_row).await.unwrap());

    let hit = repo
        .get_diagnosis_cache(&hash, "2026-11-05T10:00:00.000Z")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hit.hits, 1);

    // Expirado retorna None
    let expired = repo
        .get_diagnosis_cache(&hash, "2026-12-05T10:00:00.000Z")
        .await
        .unwrap();
    assert!(expired.is_none());

    let cleared = repo.clear_cache(CacheClearScope::All).await.unwrap();
    assert_eq!(cleared, 2);

    db.shutdown().await.unwrap();
}
