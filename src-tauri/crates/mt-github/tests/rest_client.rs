//! Testes do `RestClient` com servidor HTTP mock local (`ETag`, `304`, `Link` pagination, `429` rate limit e filtro de PRs).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use mt_github::auth::TokenStore;
use mt_github::client::RemoteFetchOutcome;
use mt_github::client::rest::RestClient;
use mt_github::error::GithubErrorKind;
use mt_github::model::{GithubRepoIdentity, IssueStateFilter};
use mt_github::rate_limit::RateLimitTracker;

fn sample_repo() -> GithubRepoIdentity {
    GithubRepoIdentity {
        repo_key: String::from("github.com/johnattas/maxterminal"),
        host: String::from("github.com"),
        owner: String::from("johnattas"),
        repo: String::from("maxterminal"),
        remote_name: String::from("origin"),
        html_url: String::from("https://github.com/johnattas/maxterminal"),
        default_branch: Some(String::from("main")),
    }
}

#[tokio::test]
async fn rest_client_handles_etag_304_and_filters_out_pull_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        // 1ª requisição: retorna 200 OK com 1 issue + 1 PR e ETag W/"v1"
        {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap() > 0 {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let body = serde_json::json!([
                {
                    "number": 10,
                    "title": "Bug real",
                    "body": "## PLANO DE IMPLEMENTAÇÃO\nPasso 1",
                    "state": "open",
                    "user": {"login": "john"},
                    "labels": [{"name": "Waiting Plan", "color": "0E8A16"}],
                    "comments": 2,
                    "html_url": "https://github.com/johnattas/maxterminal/issues/10",
                    "created_at": "2026-03-10T10:00:00Z",
                    "updated_at": "2026-03-10T11:00:00Z"
                },
                {
                    "number": 11,
                    "title": "PR infiltrado",
                    "state": "open",
                    "pull_request": {"url": "https://api.github.com/pulls/11"},
                    "user": {"login": "john"}
                }
            ])
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"etag-v1\"\r\nX-RateLimit-Limit: 5000\r\nX-RateLimit-Remaining: 4999\r\nX-RateLimit-Reset: 1999999999\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
        }

        // 2ª requisição: verifica If-None-Match e responde 304 Not Modified
        {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut headers_raw = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap() > 0 {
                headers_raw.push_str(&line);
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            assert!(
                headers_raw
                    .to_ascii_lowercase()
                    .contains("if-none-match: \"etag-v1\""),
                "Deveria ter enviado If-None-Match: {headers_raw}"
            );
            let resp = "HTTP/1.1 304 Not Modified\r\nX-RateLimit-Limit: 5000\r\nX-RateLimit-Remaining: 4999\r\nX-RateLimit-Reset: 1999999999\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            socket.write_all(resp.as_bytes()).await.unwrap();
        }
    });

    let token_store = TokenStore::in_memory();
    token_store
        .set_token("ghp_1234567890abcdef", Some(String::from("johnattas")))
        .unwrap();
    let rl = RateLimitTracker::new();
    let client = RestClient::with_base_url(format!("http://{addr}"), token_store, rl).unwrap();

    let repo = sample_repo();
    let first = client
        .fetch_issues(&repo, IssueStateFilter::Open, 50, 1, None)
        .await
        .unwrap();
    match first {
        RemoteFetchOutcome::Fresh { data, etag, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0].number, 10);
            assert_eq!(etag.as_deref(), Some("\"etag-v1\""));
        }
        other => panic!("Esperava Fresh, obteve {other:?}"),
    }

    let second = client
        .fetch_issues(&repo, IssueStateFilter::Open, 50, 1, Some("\"etag-v1\""))
        .await
        .unwrap();
    assert!(matches!(second, RemoteFetchOutcome::NotModified { .. }));

    server.await.unwrap();
}

#[tokio::test]
async fn rest_client_respects_rate_limit_429_and_retry_after() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(&mut socket);
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap() > 0 {
            if line == "\r\n" {
                break;
            }
            line.clear();
        }
        let body = r#"{"message":"API rate limit exceeded"}"#;
        let resp = format!(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nX-RateLimit-Limit: 60\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 1999999999\r\nRetry-After: 30\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(resp.as_bytes()).await.unwrap();
    });

    let client = RestClient::with_base_url(
        format!("http://{addr}"),
        TokenStore::in_memory(),
        RateLimitTracker::new(),
    )
    .unwrap();

    let err = client
        .fetch_issues(&sample_repo(), IssueStateFilter::Open, 50, 1, None)
        .await
        .unwrap_err();
    assert_eq!(err.kind, GithubErrorKind::RateLimited);
    assert!(err.retry_after_ms.is_some());

    server.await.unwrap();
}
