//! Testes de integração de paginação keyset e cache de snapshots de busca (`mt-db`).

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use mt_db::{DbConfig, DbError, KeysetCursor, KeysetPage, SearchSnapshotCache, open_writer};
use tempfile::TempDir;

const SCOPE: &str = "timeline_items";
const FILTER_HASH: &str = "all";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineItem {
    id: i64,
    sort_ts: i64,
    label: String,
}

fn fetch_timeline_page(
    conn: &rusqlite::Connection,
    cursor: Option<&str>,
    limit: usize,
) -> mt_db::DbResult<KeysetPage<TimelineItem>> {
    let decoded = match cursor {
        Some(raw) => Some(KeysetCursor::decode(raw, SCOPE, FILTER_HASH)?),
        None => None,
    };

    let fetch_limit = i64::try_from(limit + 1).unwrap();
    let mut rows: Vec<TimelineItem> = Vec::new();

    if let Some(ref c) = decoded {
        let mut stmt = conn
            .prepare(
                "SELECT id, sort_ts, label
                 FROM timeline_items
                 WHERE sort_ts <= ?1
                   AND ((sort_ts < ?2) OR (sort_ts = ?2 AND id < ?3))
                 ORDER BY sort_ts DESC, id DESC
                 LIMIT ?4;",
            )
            .unwrap();
        let iter = stmt
            .query_map(
                rusqlite::params![c.watermark, c.sort_key, c.tie_breaker, fetch_limit],
                |r| {
                    Ok(TimelineItem {
                        id: r.get(0)?,
                        sort_ts: r.get(1)?,
                        label: r.get(2)?,
                    })
                },
            )
            .unwrap();
        for item in iter {
            rows.push(item.unwrap());
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, sort_ts, label
                 FROM timeline_items
                 ORDER BY sort_ts DESC, id DESC
                 LIMIT ?1;",
            )
            .unwrap();
        let iter = stmt
            .query_map([fetch_limit], |r| {
                Ok(TimelineItem {
                    id: r.get(0)?,
                    sort_ts: r.get(1)?,
                    label: r.get(2)?,
                })
            })
            .unwrap();
        for item in iter {
            rows.push(item.unwrap());
        }
    }

    let has_more = rows.len() > limit;
    if has_more {
        rows.truncate(limit);
    }

    let watermark = decoded
        .as_ref()
        .map_or_else(|| rows.first().map_or(0, |r| r.sort_ts), |c| c.watermark);

    let next_cursor = if has_more {
        rows.last().map(|last| {
            KeysetCursor::new(SCOPE, FILTER_HASH, watermark, last.sort_ts, last.id).encode()
        })
    } else {
        None
    };

    Ok(KeysetPage {
        items: rows,
        next_cursor,
        has_more,
    })
}

#[test]
fn keyset_pagination_three_pages_with_concurrent_mutations() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let conn = open_writer(&cfg).unwrap().conn;

    conn.execute_batch(
        "CREATE TABLE timeline_items (
            id INTEGER PRIMARY KEY,
            sort_ts INTEGER NOT NULL,
            label TEXT NOT NULL
        ) STRICT;
        CREATE INDEX idx_timeline_sort ON timeline_items (sort_ts DESC, id DESC);",
    )
    .unwrap();

    // Insere 25 registros iniciais (id 1..=25, com alguns timestamps empatados para testar o tie-breaker).
    for idx in 1..=25_i64 {
        let sort_ts = 1_000 + (idx / 2) * 10;
        conn.execute(
            "INSERT INTO timeline_items (id, sort_ts, label) VALUES (?1, ?2, ?3);",
            rusqlite::params![idx, sort_ts, format!("item-{idx}")],
        )
        .unwrap();
    }

    // Página 1 (10 itens)
    let page1 = fetch_timeline_page(&conn, None, 10).unwrap();
    assert_eq!(page1.items.len(), 10);
    assert!(page1.has_more);
    assert!(page1.next_cursor.is_some());

    // Entre a página 1 e a página 2:
    // - Insere 3 novos itens no topo (acima do watermark da página 1)
    for new_id in 100..=102_i64 {
        conn.execute(
            "INSERT INTO timeline_items (id, sort_ts, label) VALUES (?1, ?2, ?3);",
            rusqlite::params![new_id, 9_999_i64, format!("new-top-{new_id}")],
        )
        .unwrap();
    }
    // - Deleta o item id=20 (que já saiu na página 1)
    conn.execute("DELETE FROM timeline_items WHERE id = 20;", [])
        .unwrap();

    // Página 2 (10 itens)
    let page2 = fetch_timeline_page(&conn, page1.next_cursor.as_deref(), 10).unwrap();
    assert_eq!(page2.items.len(), 10);
    assert!(page2.has_more);
    assert!(page2.next_cursor.is_some());

    // Página 3 (5 itens restantes)
    let page3 = fetch_timeline_page(&conn, page2.next_cursor.as_deref(), 10).unwrap();
    assert_eq!(page3.items.len(), 5);
    assert!(!page3.has_more);
    assert!(page3.next_cursor.is_none());

    // Verifica que todas as 25 linhas originais foram lidas sem repetição e sem incluir os novos do topo.
    let mut all_ids: Vec<i64> = page1
        .items
        .iter()
        .chain(page2.items.iter())
        .chain(page3.items.iter())
        .map(|i| i.id)
        .collect();
    assert_eq!(all_ids.len(), 25);

    all_ids.sort_unstable();
    let expected: Vec<i64> = (1..=25).collect();
    assert_eq!(all_ids, expected);
}

#[test]
fn invalid_cursor_rejected() {
    let err1 = KeysetCursor::decode("", SCOPE, FILTER_HASH).unwrap_err();
    assert!(matches!(err1, DbError::CursorInvalid { .. }));

    let err2 = KeysetCursor::decode("%%%not-valid%%%", SCOPE, FILTER_HASH).unwrap_err();
    assert!(matches!(err2, DbError::CursorInvalid { .. }));

    let valid = KeysetCursor::new("other_scope", FILTER_HASH, 100, 50, 1).encode();
    let err3 = KeysetCursor::decode(&valid, SCOPE, FILTER_HASH).unwrap_err();
    assert!(matches!(err3, DbError::CursorInvalid { .. }));
}

#[test]
fn search_snapshot_cache_lifecycle() {
    let cache: SearchSnapshotCache<i64> =
        SearchSnapshotCache::new(4, 1024 * 1024, Duration::from_millis(45));

    let page1 = cache.insert_and_paginate("q_hash_1", vec![101, 102, 103, 104, 105], 128, 2);
    assert_eq!(page1.items, vec![101, 102]);
    assert!(page1.has_more);
    let cursor1 = page1.next_cursor.unwrap();

    let page2 = cache.get_page(&cursor1, "q_hash_1", 2).unwrap();
    assert_eq!(page2.items, vec![103, 104]);
    assert!(page2.has_more);
    let cursor2 = page2.next_cursor.unwrap();

    let page3 = cache.get_page(&cursor2, "q_hash_1", 2).unwrap();
    assert_eq!(page3.items, vec![105]);
    assert!(!page3.has_more);
    assert!(page3.next_cursor.is_none());

    // Invalidação global
    cache.invalidate_all();
    assert!(cache.get_page(&cursor1, "q_hash_1", 2).is_err());

    // Expiração por TTL
    let ttl_page = cache.insert_and_paginate("q_ttl", vec![1, 2, 3], 64, 1);
    let ttl_cursor = ttl_page.next_cursor.unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert!(cache.get_page(&ttl_cursor, "q_ttl", 1).is_err());
}
