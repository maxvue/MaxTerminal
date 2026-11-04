# Plano de execução — F12 e posição persistente do inspetor

Data: 04/11/2026. Projeto: MaxTerminal. Worktree: `wt-d489f358`.
Raiz obrigatória: `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-d489f358`.
Especificação: `docs/plans/wt-d489f358/spec.md`.

## Objetivo e contrato

Adicionar F12 simples para abrir/trazer à frente o Web Inspector e lembrar localmente o modo acoplado/separado escolhido pelo usuário. Preservar inspeção do elemento pelo menu contextual, PTYs, F5, demais atalhos, layout Vue e disponibilidade atual de DevTools. Não implementar seletor automático, alternância fechar/abrir, dock lateral personalizado ou persistência de coordenadas.

A solução é exclusivamente nativa Linux. As APIs públicas investigadas comprovam acoplamento/separação, não dock em qualquer lado. Caso a interface real apresente outros modos relevantes, revalidar a especificação antes de reduzir o comportamento oferecido ao usuário. Não considerar o relato de abertura inferior como reprodução já realizada.

## Evidências e limites

Leitura confirmou raiz da worktree e status Git inicialmente limpo. Fontes, versões, contratos e arquivos existentes estão enumerados na spec, seção 3. Nenhuma instalação, teste, build, abertura do aplicativo ou mudança de código foi feita. Não foi verificada a sequência de sinais em execução, a ordem do teclado GTK ou a versão da biblioteca de sistema carregada pelo binário. Essas lacunas são gates reais de integração.

Mudanças previstas: existentes `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, condicionalmente `src-tauri/Cargo.lock`; novo `src-tauri/src/devtools.rs` e testes internos nesse módulo. Não há alteração prevista em Tauri config, capabilities, bindings, frontend ou banco.

## Decisões e justificativas

- Usar WebKitGTK público: é o dono do inspetor e cobre também o menu contextual; listener Vue isolado não resolveria a persistência.
- F12 abre, não alterna fechamento; usuário pediu inspeção, não toggle.
- Guardar preferência em arquivo nativo pequeno dentro de `AppPaths.config_dir`, evitando banco, IPC ou frontend para uma preferência do toolkit. Respeitar `MAXTERMINAL_HOME`.
- Não persistir `detach` indiscriminadamente, pois também ocorre no fechamento. Diferenciar intenção do usuário de modo efetivo e restauração.
- Não habilitar feature de DevTools em release por inferência. A política atual é preservada; expansão para distribuição requer requisito explícito.
- Sem frontend, não criar componentes, stores ou helpers Vue nem alterar MaxComponentsUi/MaxPinia/MaxUse. Se essa premissa mudar, revisar o plano e investigar os exports reais dessas bibliotecas.
- Não trocar WebView, alterar CSP ou usar JavaScript privado do inspetor.

## Tarefas ordenadas

### T1 — Preflight por leitura e contratos nativos

1. Ler este plano e a spec na primeira admissão, confirmar branch/worktree sem trocar ou criar outra e conferir diff/status para preservar trabalho existente.
2. Conferir manifests, lockfile, fontes locais e `cfg` efetivos. Confirmar as dependências diretas GTK/WebKitGTK compatíveis com o lock, reexports, `gtk_window`, `with_webview`, eventos de teclado e sinais do inspetor.
3. Inspecionar scripts, pre/post-hooks, aliases e symlinks das bibliotecas locais antes de qualquer validação. Confirmar integridade dos artefatos e identidades resolvidas de Vue, Axios e Pinia do app/harness/libs, se o check canônico depender delas. Não reparar por sync/instalação implícita nem contornar o gate.
4. Ler os testes/bootstrap para confirmar isolamento de banco, cache, filas, storage, rede e processos. Não copiar `.env`, resetar banco, migrar ou executar teste compartilhado por supor que worktree/transação isola tudo.
5. Preparar futuramente perfil descartável via `MAXTERMINAL_HOME` absoluto e bases XDG separadas, preservando toolchain e runtime gráfico. Conferir single-instance, portas e serviços antes de abrir o aplicativo; worktree e variável de perfil sozinhas não garantem isolamento de tudo.

Saída: contratos confirmados e procedimento seguro de validação. Gate falhado bloqueia a etapa dependente, com próxima ação concreta. Não usar `npm run dev` indiscriminadamente: seu script sincroniza libs, limpa cache e gerencia processos. Há notificação separada sobre falha de sync ignorada.

### T2 — Modelo e persistência local

Arquivos: novo `src-tauri/src/devtools.rs`; manifests somente para dependências nativas necessárias.

- Implementar enumeração de modo e arquivo `devtools.json` com schema e limites da spec; defaults sem preferência mantêm comportamento nativo.
- Separar codec/IO e redutor puro de transições da ligação GTK, permitindo testes sem WebView.
- Gravar somente mudanças reais de intenção, de forma atômica e privada; não seguir symlinks. Falha de IO gera aviso sanitizado e mantém estado em memória.
- Testes: round-trip de ambos os modos; arquivo ausente, inválido, grande, versão desconhecida, symlink e falha de IO; isolamento por diretório temporário; restauração não grava; fechamento não substitui preferência; fallback de acoplamento preserva intenção.

Conclusão de implementação: todo esse código e os testes prontos, sem executar suíte entre microedições.

### T3 — F12 e integração do ciclo nativo

Dependência: T1/T2. Arquivos: módulo novo, `src-tauri/src/lib.rs`, manifests/lock se necessário.

- Instalar adaptador uma vez na janela `main`, no setup, sem modificar encerramento/supervisão de PTYs. Usar thread GTK e referências fracas para evitar ciclos de sinais/objetos; não mover objetos GTK por threads.
- Capturar F12 simples na janela GTK antes do WebView. Consumir tecla reservada, suprimir repetição por pressionar/liberar e reset em perda de foco. Considerar NumLock/CapsLock; preservar teclas modificadas. Não duplicar handler DOM ou registrar atalho global.
- Mostrar/trazer à frente o inspetor sem fechá-lo se já aberto, sem mover painel já aberto.
- Observar attach/open-window/closed e abertura comum para restaurar preferência por F12 ou contexto, protegendo recursão e gravações programáticas. `detach` sozinho não confirma escolha. Manter handlers internos de Wry funcionando.
- Completar testes de modelo para fechar acoplado, fechar separado, reabrir, restaurar, falha, transições programáticas e repetição de tecla. Testes não substituem o gate GTK.
- Manter código e imports condicionais compiláveis no release padrão, sem ativar developer extras fora da política existente.

Conclusão: escopo e testes implementados; revisar em conjunto contratos e possíveis conflitos de foco antes da bateria de validação. Execução sequencial é preferível: as tarefas compartilham módulo/manifests; não há benefício de executores concorrentes aqui.

### T4 — Validação agrupada e revisão de entrega

Depois de T2/T3 completos, revisar diff por risco (sinais, fechamento, segurança do arquivo, threads, cfg, propagação de teclas), executar `npm run check` a partir da raiz após gates de isolamento/integridade aprovados. Esse script já executa a suíte completa, tipos, build e checks Rust; não duplicar cada etapa como primeira ação. Registrar o comando e todos os estágios efetivamente executados, inclusive falhas e interrupção no primeiro erro. Corrigir falhas em lote e reexecutar os checks afetados, reutilizando resultados ainda válidos da mesma revisão.

Se houver necessidade de build nativo release adicional, definir o comando após conferir assets, hooks e configuração efetiva; o build Vite do check não comprova compilação release Rust. Não inventar flag de feature ou bypass de integridade. Ausência desse aceite deixa pendência explícita.

Realizar aceite nativo no perfil isolado após autorização operacional pertinente. Registrar versão de WebKitGTK efetivamente usada e revisão do binário. Testar a matriz abaixo; sem ambiente gráfico seguro, registrar `manualPending`, não conclusão total. Esperas assíncronas devem ter prazo e recuo, sem polling repetitivo sem informação nova.

## Matriz de aceite por tarefa

| Tarefa/requisito | Evidência e testes pertinentes | Revisão por risco | Verificação manual |
| --- | --- | --- | --- |
| T1: contratos e ambiente | Manifests, fontes fixadas, hooks e isolamento documentados | Sem mutações em perfil/serviço compartilhado | Confirmar modos oferecidos e política de build |
| T2: preferência persistente | Testes codec/IO e transições nos testes Rust do check | Schema fechado, permissões, symlinks, falha de IO | Reiniciar com o mesmo perfil descartável e confirmar ambos os modos |
| T3: F12 | Testes de filtro/repetição e estado; inspeção de handler nativo | Tecla não chega ao PTY; sem handler duplo/loop | Foco em terminal/TUI, input e modal; segurar F12; já aberto; modificadores; NumLock/CapsLock; F5 e atalhos de agentes |
| T3: contexto e fechamento | Casos attach/detach/open-window/closed no modelo | Fechar não altera intenção; restauração não grava fallback | Inspecionar nó clicado, alternar modo, fechar/reabrir por cada entrada, reload, reiniciar, repetir ciclos e redimensionar janela |
| T4: robustez e compatibilidade | `npm run check` e conferência de cfg | Nenhum aumento da superfície de DevTools no release | Sem arquivo, JSON inválido, diretório sem escrita e janela pequena; app segue funcional |

Para a sequência crítica: escolher acoplado → fechar → reabrir por contexto → confirmar acoplado; escolher separado → fechar → reabrir por F12 → confirmar separado; repetir após reload e após reiniciar. Observar ordem de sinais em instrumentação temporária sanitizada apenas se necessária, sem registrar conteúdo da página, e removê-la da entrega. Não concluir preservação contextual a partir de um dublê.

## Riscos, operação e recuperação

O maior risco é confundir detach de fechamento com escolha real ou restaurar recursivamente durante abertura. Segundo risco é a tecla alcançar xterm/PTY pela ordem GTK. Falhas nesses gates exigem diagnóstico do fluxo antes de trocar arquitetura; não iniciar fallback Vue com IPC sem revisão do plano. Não prometer dock lateral nem coordenadas onde a API não oferece suporte. Preferência inválida/inacessível degrada para padrão nativo, nunca impede startup.

Não há migração nem dados sensíveis novos. Logs guardam somente código de erro, sem valor bruto do arquivo nem dados da página. Nenhuma limpeza de perfil real é necessária. Eventual reversão da feature é por mudança revisada no módulo e integração, preservando outras alterações; não resetar branch, remover banco ou apagar configurações do usuário. O arquivo opcional pode ser ignorado por versões anteriores.

## Retomada e registro durante Execute

Este plano não autoriza implementação nesta sessão. Em Execute, ler plano/spec na primeira admissão; em retomadas consultar somente trechos pertinentes, conferir trabalho existente, revalidar revisão factual dos arquivos e seguir dependências/decisões sem reabri-las sem evidência nova. Registrar progresso nos marcos T1–T4 e no handoff em `docs/plans/wt-d489f358/execution-state.json`, exclusivamente durante Execute. Não criar esse arquivo no Plan.

Contrato version 2 estrito, sem chaves adicionais:
- Raiz: `{version: 2, planRevision, tasks, verification, updatedAt?, nextAction?}`. `planRevision` é SHA256 real de `plan.md`; `updatedAt` é inteiro positivo em milissegundos, não ISO. Proibidos `schemaVersion`, `worktree`, `branch`, `head`, `planPath`, `status` e `checks` solto.
- Tarefa: `{id, status, files, validations, revision?, nextAction?}`. IDs T1–T4; `validations: []` é sempre obrigatório. Proibidos `title` e `evidence`. Status permitido: `pending`, `running`, `completed`, `blocked`, `manualPending`, `validationFailed`. `revision`, quando presente, é SHA256 real da revisão factual do conteúdo da tarefa; documentar ordenação/serialização estável dos arquivos usados no cálculo no handoff, não rótulos arbitrários.
- `verification`: `{revision: SHA256, checks: [{command, revision: SHA256, result, exitCode?}]}`. Check somente nessas chaves e somente em `verification.checks`; comando até 4.096 caracteres. Hashes devem vincular conteúdo real validado, não rótulos.
- `validations` registra validações pertinentes da tarefa; resultados de validação e de check somente `passed`, `failed`, `pending`, nunca `blocked`. Não introduzir objetos de check duplicados fora de `verification.checks`.
- Check `passed` exige `exitCode: 0`; `failed` exige inteiro diferente de zero; `pending` não admite `exitCode`. Nunca `null`. Registrar comandos realmente executados e todas as falhas, inclusive suíte completa; não inferir exit code de metadado genérico de conclusão da ferramenta.
- `nextAction` até 512 caracteres registra bloqueios e próxima ação concreta. Proibidos em qualquer nível campos extras como `blocker`, `diagnostic`, `branchSync`, `durableSync`, `followUp`.

Checks vazios, ausência de resultado vigente ou falha vigente impedem validação integral. Compatibilidade com version 1 não prova validação integral automaticamente. Registro autodeclarado não prova execução nativa nem detecta verificações omitidas. Não declarar conclusão total enquanto houver gate pendente. Repetição da mesma falha sem progresso exige diagnóstico agrupado do harness/bootstrap antes de repetir testes equivalentes. Não criar worktree, trocar branch ou fazer commit/merge/push; integração pertence ao MaxCode mediante pedido do usuário.
