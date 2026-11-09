# Plano de implementação — Issue #18

### Descrição e Causa Raiz

**Veracidade: lacuna de cobertura confirmada por inspeção estática.** O E2E de segredos continua usando um formatador implementado pelo próprio teste, diferente da composição utilizada pela aplicação. É necessário corrigir a representatividade dos testes. Não foi comprovado vazamento em produção nem falso positivo por execução de mutante. Nenhum teste, build ou mutação foi executado nesta etapa de planejamento.

**Causa Raiz Comprovada:**

- `src-tauri/tests/sanitizer_e2e.rs:L77-96` define `RedactingFormatter`, que formata os campos como texto e chama `sanitize` diretamente. Em `L338-347`, o subscriber instala esse substituto. As verificações de ausência de segredos em `L348-359` examinam apenas a saída dele, sem atravessar o visitante, a serialização JSON ou o redator reais. Também não exigem registro presente, JSON válido ou marcador de redação, permitindo aprovação com saída vazia.
- `src-tauri/crates/mt-db/tests/sanitizer_sinks.rs:L142-161` repete a estratégia com `SanitizedEventFormatter`; `L217-239` instala e verifica esse adaptador. Nesse segundo teste já há exigência positiva de marcador, mas a composição continua diferente da aplicação.
- A aplicação seleciona `SanitizerLogRedactor` em `src-tauri/src/lib.rs:L91-118`. `src-tauri/src/logging.rs:L397-412` instala `RedactedJsonFormatter` nos dois destinos, arquivo e stderr, com o mesmo redator e filtros.
- O caminho real aplica `SafeFieldVisitor::record_str` em `src-tauri/src/logging.rs:L171-197` e a conversão de valores Debug em `L219-223`; chama `SanitizerLogRedactor::redact` em `L51-58`; e serializa o resultado em `L265-290`. Os formatadores particulares dos testes desviam de todas essas etapas.

**Rastreamento reverso dos dados:** bytes do arquivo/stderr ← writers não bloqueantes (`logging.rs:L387-408`) ← JSON (`L268-290`) ← visitante (`L265-266`, `L171-197`, `L219-223`) ← redator real (`L51-58`) ← `mt_db::sanitizer::sanitize` (`src-tauri/crates/mt-db/src/sanitizer/mod.rs:L169-175`) ← campos do evento `tracing`. A escolha dessa política parte de `run()` (`src-tauri/src/lib.rs:L118-124`). No teste atual, os bytes vêm de `RedactingFormatter → sanitize`, sem passar pela composição acima. UI, store e rotas não participam da reprodução; SQLite é outro destino validado pelo mesmo E2E, não uma etapa intermediária do logger. Para eventos do frontend, o comando IPC `app_log_frontend` delega a `record_frontend_event_with_now`, que emite `tracing` em `src-tauri/src/commands/app.rs:L24-55`.

**Cobertura preexistente e limites do achado:** `src-tauri/src/logging.rs:L588-596` e `L682-701` testam o formatador com `FoundationRedactor`. Entretanto, já existem testes com a composição real em `src-tauri/src/startup.rs:L102-107` e `L143-164`; o segundo verifica uma sentinela Bearer. Portanto, não é correto afirmar ausência total de cobertura dessa composição. Esses casos não substituem o E2E multissink nem a matriz das 13 famílias. A necessidade comprovada é corrigir essa lacuna específica, sem pressupor defeito no sanitizador.

### Arquivos afetados

1. `src-tauri/tests/sanitizer_e2e.rs`: substituir o formatador particular pela composição real e fortalecer as asserções do trecho de logs.
2. `src-tauri/tests/sanitizer_logging.rs` (novo): testes de integração da composição real com matriz de segredos, campos e controles positivos/negativos.
3. `src-tauri/crates/mt-db/tests/sanitizer_sinks.rs`: conservar a matriz de persistência e payload externo; retirar o adaptador de logging particular e atualizar descrição/nome do teste ao escopo restante.

O plano foi armazenado em `docs/issues/18/plan.md`. As referências de produção acima são evidências e pontos para mutação temporária na futura validação, não arquivos previstos para alteração permanente. As APIs já são públicas (`src-tauri/src/lib.rs:L31`, `src-tauri/src/logging.rs:L49`, `L227-235`), e as dependências necessárias já constam no pacote `maxterminal`.

### Execuções propostas

1. Executar o bloco de implementação em worktree dedicada, preservando alterações preexistentes. Implementar todos os ajustes e seus testes antes da primeira rodada de testes, tipos ou build.
2. No E2E, importar `RedactedJsonFormatter`, `SanitizerLogRedactor` e `build_env_filter` de `maxterminal_lib::logging`; remover `RedactingFormatter` e seus imports exclusivos. Manter o writer em memória, que apenas captura os bytes.
3. Configurar o subscriber local com `RedactedJsonFormatter::new(Arc::new(SanitizerLogRedactor))` e o filtro real com nível explícito. Emitir o evento com `target: "maxterminal"` e payload bruto. O target explícito é indispensável: o target padrão do teste não integra o filtro de produção (`logging.rs:L365-378`).
4. Capturar o evento dentro de `tracing::subscriber::with_default`, sem `await` nessa região. Exigir exatamente o número esperado de linhas, JSON válido, nível/target corretos, texto diagnóstico preservado e marcadores correspondentes aos segredos. Verificar ausência dos segredos tanto nos bytes capturados quanto nos valores JSON decodificados. Não pré-sanitizar a entrada nem calcular o resultado esperado chamando o próprio sanitizador.
5. Criar `sanitizer_logging.rs` com fixtures sintéticas locais, representando todas as variantes de `SecretKind::ALL`, usando os exemplos de `sanitizer_sinks.rs:L32-116` como referência. Conferir igualdade dos conjuntos de famílias, não apenas quantidade. Manter dados de teste independentes de detalhes privados de `mt-db`.
6. Nessa suíte, exercitar texto em `message`, campo livre `payload` e campos conhecidos `event`, `version`, `code` e `command`; cobrir registro textual direto, Display (`%`) e Debug (`?`). Incluir segredos multilinha/PEM em mensagem e payload Display e um caso Debug com aspas/barras, verificando a saída JSON decodificada. Não duplicar a chave `message` no mesmo evento. Cada caso precisa comprovar chegada ao sink, marcador correto e preservação de uma sentinela benigna.
7. Acrescentar controles: evento benigno deve permanecer legível; target não autorizado deve ser filtrado enquanto um evento autorizado da mesma captura permanece; campos numéricos suportados, como `dropped`, devem manter o tipo. Usar o filtro real e subscriber local, sem alterar variáveis de ambiente, subscriber global ou panic hook.
8. Em `mt-db/tests/sanitizer_sinks.rs`, retirar somente a seção de logs simulados, writer, formatador e imports que ficarem exclusivos deles. Preservar os 13 casos, marcadores, verificações SQLite, bytes DB/WAL e payload externo. Atualizar o nome para `all_13_secret_families_are_redacted_across_sqlite_and_external_payload` e indicar no cabeçalho que a composição de logs é coberta no pacote da aplicação. Não introduzir dependência reversa de `mt-db` para `maxterminal`.
9. Validar o lote completo, consolidar falhas, corrigir em lote e repetir a validação pertinente. Se a composição real revelar um defeito funcional, registrar o caso reproduzível e estabelecer a causa antes de ampliar o escopo para código de produção.

### Testes de regressão e validação em lote

**Bloco automatizado:**

- E2E existente: conservar as verificações de chunks fragmentados, SQLite, FTS5, arquivos DB/WAL, backup e prompt Inspecionar; exigir evidência positiva do log JSON sanitizado.
- Nova suíte: 13 famílias, entrada bruta, marcadores esperados, ausência dos segredos, preservação do conteúdo benigno, campos conhecidos/livres, Display/Debug/texto, JSON válido e filtros reais. Assertivas devem identificar família e campo usando somente fixtures sintéticas.
- Não regressão: rodar os testes de logging com `FoundationRedactor`, os testes de startup que já usam o redator real e o teste de sinks remanescente de `mt-db`.
- Captura síncrona em memória não certifica flush, rotação ou entrega dos writers assíncronos; o objetivo aqui é verificar o formatador e o redator efetivamente usados pelos dois destinos. Não chamar `init_logging` dentro do processo compartilhado de testes.

**Prova por mutação, somente durante a implementação:** após o lote normal passar, em worktree temporária isolada, trocar exclusivamente o retorno de `SanitizerLogRedactor::redact` (`logging.rs:L52-54`) por `Cow::Borrowed(input)`, mantendo o motor `mt_db::sanitizer` intacto. Executar os alvos `sanitizer_e2e` e `sanitizer_logging` separadamente: ambos devem falhar nas asserções de segredo/marcador. Falha de compilação, teste filtrado ou erro de infraestrutura não conta como detecção. Uma segunda mutação, substituindo a política pelo modo restritivo no setup do teste, deve ser detectada pela exigência de conteúdo benigno e marcadores específicos; isso evita que ocultação integral seja confundida com sanitização correta. Restaurar apenas os trechos da mutação, verificar o diff e repetir o lote normal. Não versionar mutantes.

Para demonstrar empiricamente a limitação antiga, executar futuramente o E2E anterior à correção contra a primeira mutação numa worktree isolada do snapshot anterior: expectativa a verificar é aprovação desse alvo específico. Não extrapolar para a suíte inteira, pois o teste de startup pode detectar a mutação. Até essa execução, registrar apenas lacuna comprovada por código, sem declarar falso positivo confirmado.

### Banco de dados

Nenhuma migration. Schema, índices FTS5, modo WAL e contratos de persistência permanecem os mesmos; seus testes existentes devem continuar passando.

### Riscos de quebra e Não-Regressão

- **Aprovação com log vazio:** filtro real pode descartar target implícito. Prevenir com target explícito, contagem exata e conteúdo benigno obrigatório.
- **JSON escapado ocultar comparação inadequada:** verificar bytes e strings decodificadas; exigir marcadores por família, não somente ausência de substrings.
- **Redação total mascarar perda de diagnóstico:** exigir preservação do texto benigno e de metadados esperados.
- **Perda de cobertura ao retirar o formatador simulado:** adicionar a matriz real antes de retirar a seção antiga; manter as 13 famílias nos destinos SQLite/payload e nos logs da aplicação.
- **Acoplamento circular:** a aplicação depende de `mt-db` (`src-tauri/Cargo.toml:L130-134`); manter os testes de composição na aplicação, sem dependência reversa ou cópia do código de produção.
- **Interferência entre testes:** subscriber local e emissão síncrona; não usar configuração global ou mutações concorrentes no mesmo checkout.
- **Contrato de produção:** nenhuma mudança permanente no logger, IPC ou banco está prevista. Os testes de `FoundationRedactor` e startup continuam necessários e não devem ser substituídos.

### Validação

Executar na worktree de implementação, após concluir o bloco autorizado:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --test sanitizer_e2e --test sanitizer_logging
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db --test sanitizer_sinks
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --lib logging::tests
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal --lib startup::tests
rustfmt --edition 2024 --check src-tauri/tests/sanitizer_e2e.rs src-tauri/tests/sanitizer_logging.rs src-tauri/crates/mt-db/tests/sanitizer_sinks.rs
```

**Critério conclusivo:** lote normal aprovado; ambos os alvos de integração falham por asserção sob o bypass temporário do redator real; logs vazios/ocultados não satisfazem os controles positivos; lote volta a passar após restaurar a mutação. Registrar comandos, resultados e diff final. Uma eventual indisponibilidade do toolchain ou de bibliotecas nativas Tauri deve ser relatada como bloqueio de validação, não como sucesso nem como confirmação do bug.

### Skills Aplicáveis

- `code-review`: revisar representatividade dos testes, direção de dependências e provas de regressão; usada na análise deste plano.
- `security-and-hardening`: aplicar durante a implementação dos testes de fronteira de logs, usando segredos exclusivamente sintéticos e verificando sanitização antes da serialização.
- `test-driven-development`: orientar as asserções comportamentais e a prova de sensibilidade por mutação; observar a exigência específica desta tarefa de concluir todo o bloco autorizado antes da validação em lote.

Skills de Vue/UI não se aplicam ao ajuste de testes Rust descrito neste plano.
