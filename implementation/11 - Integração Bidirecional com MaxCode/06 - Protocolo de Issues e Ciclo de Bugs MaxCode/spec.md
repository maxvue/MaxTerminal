# Implementação 06 — Protocolo de Issues e Ciclo de Bugs MaxCode (`spec.md`)

> Fase 11 · Decisão **D-03**, achado **C-03** · Depende de **11/01–05**.
> Contrato **proposto**, condicionado à validação da versão real do MaxCode e às capabilities negociadas. Fixtures provam o lado local, não interoperabilidade.

## 1. Escopo e ownership

Entregar em `mt-bridge` o transporte tipado de **Abrir**, **Analisar (`BUG PLAN`)**, **Corrigir (`BUG FIX`)** e do ciclo delegado **Corrigir todos os Bugs**. A Fase 16 consome exclusivamente por seu adaptador de `MaxCodePort`; não modifica arquivos da 11. Esta unidade não importa `mt-github`, não implementa fila de correção, catálogo de labels, acesso ao GitHub ou persistência de ciclos. `ConversationMode::Plan` e o comando genérico `maxcode_open_plan` continuam independentes das ações de issues.

### Arquivos da implementação futura

| Ação | Caminho | Entrega |
| :-- | :-- | :-- |
| Criar | `src-tauri/crates/mt-bridge/src/issues.rs` | DTOs de transporte, validação, suporte efetivo e projeções de ACK/progresso |
| Modificar | `src-tauri/crates/mt-bridge/src/protocol.rs` | Extensões aditivas de `Capability`, `MessageType`, `Message`, `AckPayload` e decoder |
| Modificar | `src-tauri/crates/mt-bridge/src/client.rs` | Métodos públicos abaixo; seleção de peer, gate, envio e reconciliação |
| Modificar | `src-tauri/crates/mt-bridge/src/uri.rs` | `MaxCodeUri::Issue` e builder condicionado pelo adaptador |
| Modificar | `src-tauri/crates/mt-bridge/src/session.rs` | Dispatch de progresso autenticado, correlação e assinatura limitada |
| Modificar | `src-tauri/crates/mt-bridge/src/lib.rs` | `pub mod issues` e reexports |
| Criar | `src-tauri/crates/mt-bridge/fixtures/maxcode/issues-protocol.v1.json` | Handshake, comandos, ACKs, progresso e casos incompatíveis sanitizados |
| Criar | `src-tauri/crates/mt-bridge/tests/issues_protocol_it.rs` | Contrato local com peer fake, capabilities e identidade de entrega |
| Criar | `src-tauri/crates/mt-bridge/tests/bugs_cycle_it.rs` | Prévia/start/status/progresso, ACK perdido e reconexão |

Nenhuma migration, novo comando Tauri ou novo evento público `bridge://*` é necessário. A 16 oferece os comandos/eventos `github_*`/`github://*` e sua migration `0010_16_github.sql`. A ponte fornece assinatura Rust interna de progresso.

## 2. Gate bilateral obrigatório

- Adicionar `Capability::IssuesProtocolV1` → `issues_protocol_v1` e `Capability::BugsCycleV1` → `bugs_cycle_v1` no handshake. O adaptador oferece cada uma somente após validação registrada por versão real da extensão. O suporte efetivo é a interseção entre validação local do adaptador e capabilities do peer autenticado associado ao projeto.
- `issues_protocol_v1` habilita `open/plan/fix`; `bugs_cycle_v1` habilita preview/start/status/progress. Ausência, `Unknown` ou incompatibilidade desabilita a ação com motivo pt-BR (`bridge.unsupported_capability`), sem enviar frame, abrir URI ou simular sucesso. Nunca transformar `fix` em `open_plan`.
- Negociar também o `catalogHash` validado para planejamento/correção/ciclo. O hash é metadado da compatibilidade bilateral; a 16 o compara com seu catálogo, a 11 o transporta/verifica sem conhecer labels. Extensão do `HelloPayload`: campos opcionais `issuesCatalogHash` e `bugsCycleDurableResume`, definidos explicitamente por Serde. Ausência do hash bloqueia plan/fix/ciclo; `open` pode permanecer disponível.
- `bugsCycleDurableResume: true` só é aceito quando a versão real comprovou reconciliação durável por requestId/cycleId, inclusive após reinício do MaxCode. É gate adicional de `start`; a capability genérica `idempotent_requests` e seu cache de cinco minutos não provam durabilidade. Sem esse contrato, mostrar indisponibilidade do início de lote.
- O suporte a `/issue` exige validação separada do handler URI como **wake-only**. Uma versão que execute a ação diretamente pela URI é incompatível com este fluxo. Conexão perdida invalida suporte anunciado pela sessão; wake de peer desconectado só é permitido com perfil de versão previamente validado, rechecando capabilities após handshake antes do envio.

## 3. Mensagens e DTOs

Reutilizar `Envelope` v1, autenticação, limites (256 KiB, profundidade 32), `senderId`, `id`, `reply_to`, `correlationId` e erros `bridge.*` de 11/01–02. Payloads novos usam `#[serde(rename_all = "camelCase")]`; enums usam nomes explícitos abaixo. Não mudar nomes de fio dos payloads antigos.

| `type` / variante | Direção | Payload | Confirmação |
| :-- | :-- | :-- | :-- |
| `issue.command` / `IssueCommand` | MT → MC | `IssueCommandRequest` | `ack` com `issueReceipt` |
| `bugs.cycle.preview` / `BugsCyclePreview` | MT → MC | `BugCyclePreviewRequest` | `ack` com `cyclePreview` autoritativa |
| `bugs.cycle.start` / `BugsCycleStart` | MT → MC | `BugCycleStartRequest` | `ack` com `cycleProgress` (`accepted` ou estado real) |
| `bugs.cycle.status` / `BugsCycleStatus` | MT → MC | `BugCycleStatusRequest` | `ack` com `cycleProgress`, inclusive `unknown` quando não reconciliável |
| `bugs.cycle.progress` / `BugsCycleProgress` | MC → MT | `BugCycleProgress` | Evento autenticado, sem gerar nova ação |

```rust
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueAction { Open, Plan, Fix }

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommandRequest {
    pub action: IssueAction,
    pub repo: String,
    pub issue_number: u32,
    pub project_path: PathBuf,
    pub worktree: Option<PathBuf>,
    pub correlation_id: Uuid,
    pub catalog_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCyclePreviewRequest {
    pub repo: String,
    pub project_path: PathBuf,
    pub catalog_hash: String,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleStartRequest {
    pub repo: String,
    pub project_path: PathBuf,
    pub preview_id: String,
    pub catalog_hash: String,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleStatusRequest {
    pub repo: String,
    pub project_path: PathBuf,
    pub correlation_id: Uuid,
    pub cycle_id: Option<String>,
}
```

`repo` é `owner/name` validado, sem URL/credencial. O v1 atende `github.com`; outra autoridade exige perfil bilateral validado, não descartar `GithubRepoRef.host` silenciosamente. `issueNumber > 0`; `projectPath` absoluto/cadastrado; worktree opcional com vínculo Git comprovado por 11/03. UUIDs são válidos; novas intenções locais usam v7. Paths ≤4096 bytes, repo ≤256, IDs opacos de preview/ciclo/operação ≤256, sem controles; hashes são SHA-256 hex de 64 caracteres. Nunca enviar corpo de issue, prompt, token ou comando shell nesta API.

### Respostas tipadas (todas structs Serde camelCase + Specta)

- `IssueReceipt { correlation_id: Uuid, state: IssueDeliveryState, peer_operation_id: Option<String>, message: Option<String> }`; `IssueDeliveryState` serializa `accepted | rejected | unknown`. `accepted` significa ação aceita pelo handler correto, não bug corrigido. `pending` é estado local da 16 antes do ACK. `accepted` exige `peerOperationId`; rejeição exige motivo pt-BR sanitizado.
- `BugStageCount { stage: String, label: String, count: u32 }`; contrato alinhado ao consumidor 16/03. Etapa e label vêm do MaxCode, nunca inferidas pela ordem das labels; se o perfil externo não fornecer a label exigida, recusar resposta incompatível em vez de inventá-la.
- `BugCyclePreview { correlation_id: Uuid, preview_id: String, repo: String, catalog_hash: String, total_candidates: u32, deduplicated_count: u32, stages: Vec<BugStageCount>, selection_criteria: String, expires_at_ms: i64 }`. Prévia é autoritativa e não inicia tarefa.
- `BugCycleProgress { correlation_id: Uuid, cycle_id: Option<String>, status: BugCycleStatus, peer_revision: u32, total: Option<u32>, stages: Vec<BugStageCount>, updated_at_ms: i64 }`; `BugCycleStatus` serializa `pending | accepted | running | unknown | completed | failed | cancelled`.
- `AuthenticatedBugCycleProgress { peer_id: Uuid, sender_id: Uuid, progress: BugCycleProgress }` é wrapper Rust interno: identidade provém da sessão, nunca do payload. A 16 conserva vínculo com o peer persistido e trata `observation_stale` localmente.

Adicionar a `AckPayload` campos opcionais `issue_receipt: Option<IssueReceipt>`, `cycle_preview: Option<BugCyclePreview>`, `cycle_progress: Option<BugCycleProgress>`, com `#[serde(default, skip_serializing_if = "Option::is_none", rename = "...")]` usando respectivamente `issueReceipt`, `cyclePreview`, `cycleProgress`. Um ACK desta extensão carrega exatamente a resposta esperada para o pedido, não várias. `handled:true` sem resposta tipada não confirma a ação. `error` mantém envelope e códigos da ponte. `CachedResult::Completed(AckPayload)` existente comporta a extensão sem cache paralelo.

Validar `reply_to`, mesmo peer/sender, repo esperado, hash, `correlationId` externo/interno iguais e IDs da prévia/ciclo. `stages` ≤32, mensagem ≤2000 caracteres, `selectionCriteria` ≤4096 bytes; timestamps em milissegundos seguros para TS. Eventos antigos não regridem estado; mesma revisão é repetição idempotente e conteúdo divergente na mesma revisão é erro. Snapshot terminal só é aceito do peer autenticado responsável pelo ciclo; não pressupor assinatura criptográfica de payload inexistente.

## 4. API pública do cliente e entrega

```rust
impl MaxCodeClient {
    pub fn issue_protocol_support(&self, project_path: &Path) -> IssueProtocolSupport;
    pub async fn send_issue_command(&self, request: IssueCommandRequest) -> Result<IssueReceipt, BridgeError>;
    pub async fn bugs_cycle_preview(&self, request: BugCyclePreviewRequest) -> Result<BugCyclePreview, BridgeError>;
    pub async fn bugs_cycle_start(&self, request: BugCycleStartRequest) -> Result<BugCycleProgress, BridgeError>;
    pub async fn bugs_cycle_status(&self, request: BugCycleStatusRequest) -> Result<BugCycleProgress, BridgeError>;
    pub fn subscribe_bug_cycle_progress(&self) -> tokio::sync::broadcast::Receiver<AuthenticatedBugCycleProgress>;
}
```

`IssueProtocolSupport` é snapshot Rust de flags `can_open_issue`, `can_plan_issue`, `can_fix_issue`, `can_preview_bug_cycle`, `can_start_bug_cycle`, `can_query_bug_cycle_status`, `supports_uri_wakeup`, `catalog_hash: Option<String>`, `protocol_version: Option<u16>` e `unavailable_reason: Option<String>`, calculado conforme §2. A 16 projeta esse snapshot em seu DTO `IssueCapabilities`, sem inferir suporte apenas de `BridgeState::Connected`.

1. Validar pedido/gates e selecionar peer compatível do projeto/worktree exato. Múltiplos peers ambíguos → indisponibilidade, nunca broadcast. O adaptador tem padrões desabilitados para recursos externos não validados.
2. Para `issue.command`/`bugs.cycle.start`, `Envelope.id = correlation_id` da intenção persistida pelo consumidor. Retransmissões conservam id, sender e payload. Para preview/status, gerar id de mensagem novo; em status, `correlationId` identifica a intenção de start original. `correlationId` não é credencial.
3. Reutilizar request/ACK autenticado e timeout de 11/02 (padrão 3 s). `issue.command` e start entram na dedup; preview/status são leituras. Não reiniciar ciclo após timeout: `bridge.delivery_unknown` mapeia para `Unknown` na 16 e exige status. `bridge.unsupported_capability` é rejeição terminal e não dispara fallback.
4. No wake de issue, aguardar handshake por até **10 s** (orçamento específico, inferior ao TTL de 30 s da fila base); revalidar gates e enviar uma única operação com a mesma identidade. URI sem ACK nunca produz `Accepted`. Perda de ACK de issue não cria nova intenção automaticamente; consulta específica de recibo só pode existir em adaptador real validado, caso contrário manter `Unknown`.
5. `bugs_cycle_start` exige prévia não expirada e confirmação já validada pela 16; o MaxCode revalida prévia/hash. A 11 não cria fila local nem faz decisões de labels. Repetir a intenção após reinício exige reconciliação durável no MaxCode, não o cache efêmero local.
6. Progresso usa canal Rust limitado a 64 itens. Recebedor com `Lagged`, desconexão ou novo sender marca observação desatualizada e consulta status; nunca inventa conclusão. Shutdown cancela assinaturas/pedidos; reconexão não reenvia start. A persistência/reconciliação após reinício do app é da 16.

## 5. URI de issues (proposta condicionada)

Adicionar `MaxCodeUri::Issue { action: IssueAction, repo: String, number: u32, request_id: Uuid }` e `MaxCodeAdapter::build_issue_uri(&self, request: &IssueCommandRequest) -> Result<String, BridgeError>`, com implementação padrão retornando `UnsupportedCapability`. Builder só é chamado após o gate de perfil validado; usa o mesmo validador do DTO.

Formato: `vscode://johnattas.maxcode/issue?action=<open|plan|fix>&repo=<owner/name codificado>&number=<n>&requestId=<correlationId>`.

É exceção explícita à URI genérica que só transporta UUID: admite apenas esses metadados mínimos, encoding RFC 3986, limite 2 KiB, sem caminho/worktree, corpo, prompt ou token. O perfil validado deve comprovar que a URI só acorda/seleciona contexto, sem executar `plan/fix` antes do pedido autenticado. `xdg-open` segue argv sem shell. Ciclo em lote não usa URI para iniciar tarefa; requer canal compatível e confirmação da prévia.

## 6. Consumo pela Fase 16 (contrato de integração)

| Fase 16 / `MaxCodePort` | Fase 11/06 |
| :-- | :-- |
| `issue_capabilities()` | `issue_protocol_support(project_path)`; copiar flags/motivo, validar catálogo |
| `IssueCommandPayload.command` | `IssueCommandRequest.action` (`Open/Plan/Fix`) |
| `request_id: RequestId(String)` | Validar/converter UUID estável para `correlation_id`; novas intenções v7. Não gerar outro UUID em retries |
| `repo: GithubRepoRef` | Validar host/perfil, transportar `owner/name`; manter `RepoId` e auth scope no domínio 16 |
| `project_path`, `worktree`, `issue_number`, `catalog_hash` | `project_path`, `worktree`, `issue_number`, `catalog_hash` nos DTOs da ponte |
| `schema_version` | Validar `1` no adaptador; envelope `v=1`, não serializar DTO de domínio diretamente |
| `DeliveryReceipt` | Converter `IssueReceipt`; `correlationId → requestId`, preservar operação/motivo; erro após possível envio → `Unknown` |
| Preview/start/status | Converter para requests públicos e respostas; reanexar `GithubRepoRef` validado ao resultado da prévia |
| Progresso | Assinatura Rust autenticada → `BugCycleTracker` → `github://bug-cycle-progress`; `correlationId → requestId`, `peerRevision` preservado; `observationStale` local |

**Conferência com 16/03 (§3.2–3.3):** mapeamentos de ações, flags, hash, worktree, timeout de wake de 10 s e `Unknown` sem ACK coincidem. `BugCyclePreview` preserva `previewId`, `catalogHash`, `expiresAtMs`, contagens, critérios e etapas; o adaptador reanexa o `GithubRepoRef` original. Progresso preserva `cycleId`, `status`, `peerRevision`, `total`, `stages` e `updatedAtMs`, converte `correlationId → requestId` e calcula `observationStale` localmente (desconexão/Lagged → true; snapshot autenticado reconciliado → false).

**Contrato reconciliado com o consumidor:** 16/03 exige UUID válido em `RequestId(String)` no adaptador, sem trocar identidade em retries, e aceita término somente por snapshot recebido da sessão autenticada responsável; este contrato não define assinatura criptográfica do payload. A 16 persiste intenção antes de start, conserva peer/cycleId e consulta status após reinício. Não importar tipos de `mt-github` para a 11.

## 7. Critérios de aceite

- **CA-38:** mensagens/capabilities/métodos/URI definidos, decoder tipado e fixtures locais; ausência de capability ou validação impede efeitos. Fix nunca chama Plan genérico.
- **CA-39:** preview sem efeitos, start único, ACK perdido/status, revisão monotônica, progresso de peer incorreto rejeitado e retomada sem repetir start; gate de durabilidade separado da dedup efêmera.
- **CA-41:** paridade explícita com o adaptador de `MaxCodePort`, sem dependência reversa nem alteração da Fase 11 pela 16. Testes locais e validação real têm estados separados; registrar versão/fixtures reais antes de habilitar.
