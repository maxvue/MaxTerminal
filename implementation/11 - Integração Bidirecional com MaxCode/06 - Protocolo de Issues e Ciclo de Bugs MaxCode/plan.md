# Implementação 06 — Protocolo de Issues e Ciclo de Bugs MaxCode (`plan.md`)

> Contrato: [`./spec.md`](./spec.md), D-03 e critérios CA-38/39/41 da Fase 11. Executar após 11/01–05; a Fase 16 consome via `MaxCodePort`.

## 1. Sequência de implementação

1. **Congelar fixtures propostas:** criar `fixtures/maxcode/issues-protocol.v1.json` com capabilities presentes/ausentes, hash compatível/divergente, durabilidade ausente e wake-only validado/não validado. Documentar todos como fixtures locais até verificar a extensão real; padrões do adaptador desabilitados.
2. **Tipos e decoder:** criar `issues.rs`; estender `Capability`, `MessageType`, `Message`, `HelloPayload` e `AckPayload` conforme spec §2–3, preservando protocolo antigo. Validar limites, repo/host, paths/worktree, UUIDs, correlação e resposta tipada. Registrar `pub mod issues`/reexports.
3. **Cliente:** implementar `issue_protocol_support`, `send_issue_command`, `bugs_cycle_preview`, `bugs_cycle_start`, `bugs_cycle_status`; reutilizar seleção autenticada, pending ACKs, dedup, cancelamento e limites de 11/02. Testar `fix` sem chamar `open_plan`, recusa por falta de capability, ACK errado e `Unknown` após perda de ACK.
4. **Wake de issues:** adicionar `MaxCodeUri::Issue` e `MaxCodeAdapter::build_issue_uri` com default `UnsupportedCapability`. Preservar a intenção em requestId/correlationId, esperar handshake por 10 s e revalidar suporte. Testar URI só com campos permitidos, percent-encoding, limite 2 KiB e abertura de SO sem sucesso artificial.
5. **Ciclo/progresso:** dispatch de `bugs.cycle.progress` somente da sessão responsável; assinatura Rust limitada a 64. Testar prévia expirada/hash divergente, `Lagged`, revisões repetidas/antigas, peer errado, reconexão e status sem repetir start. A capacidade durável de recuperação é requisito bilateral, não inferência de teste local.
6. **Contrato consumidor:** conferir o mapeamento da spec §6 com DTOs/porta da 16 (RequestId UUID estável, worktree, repo, `BugStageCount { stage, label, count }`, `Accepted` vs conclusão, `observationStale` calculado localmente). Nesta fase usar consumidor fake: nenhum import de `mt-github`, migration ou comando Tauri novo. Registrar divergências para a fase consumidora corrigir em seus próprios arquivos.
7. **Interoperabilidade:** quando a extensão estiver disponível, registrar versão/commit, capabilities e fixtures sanitizadas de open/plan/fix/preview/start/status/progress/wake. Testar reinício real do MaxCode para validar durabilidade. Enquanto pendente, ação permanece indisponível com motivo; testes de fake não liberam capability em produção.

## 2. Verificação da implementação futura

Na raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge --test issues_protocol_it
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge --test bugs_cycle_it
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-bridge --all-targets -- -D warnings
npm run bindings
npm run typecheck
git diff --check
```

As suítes devem executar casos reais (zero testes não é aprovação). Regeneração de bindings não adiciona comandos IPC de issues à 11; a superfície IPC é da 16.

## 3. Evidências de aceite

| Critério | Evidência |
| :-- | :-- |
| CA-38 | Roundtrip de cinco mensagens, duas capabilities, DTOs camelCase, quatro métodos de envio/consulta e URI gated; ausência de suporte sem frame/efeito |
| CA-39 | Start idempotente, ACK perdido → status/Unknown, rejeição de peer errado, revisão monotônica, desconexão/Lagged sem conclusão inventada |
| CA-41 | Matriz de tradução `MaxCodePort` revisada, DAG sem dependência da 16, registro explícito de interoperabilidade pendente ou comprovada por versão |
