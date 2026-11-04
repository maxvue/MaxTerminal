# Foco de aba marca resultados como vistos sem revisão de apresentação

- Data: 04/11/2026.
- Localização: `src-tauri/src/workspace/manager.rs:1518–1553`, método `WorkspaceManager::tab_focus`.
- Evidência por leitura: após atualizar o painel em foco, o método chama `shell_service.tab_mark_seen` com `up_to_revision: None`. `src/stores/tabs.ts:377–389` chama esse comando ao focar aba. Em contraste, `src/composables/useProjectTabFocus.ts:24–29` documenta que foco sozinho não deve marcar como visto, e a confirmação deve depender de apresentação efetiva e revisão exata.
- Impacto potencial: usar esse comando apenas para restaurar a aba selecionada no boot pode reconhecer resultados/alertas antes de sua apresentação. Existe divergência entre o contrato documentado no frontend e a ação do backend.
- Limite da evidência: inspeção estática; não houve reprodução, teste ou medição em runtime nesta sessão.
- Encaminhamento: investigar a regra de confirmação de visualização em tarefa separada. O plano atual deve restaurar apenas seleção local e não introduzir chamada adicional de `tab_focus` no boot. Não alterar essa regra fora do escopo autorizado.
- Este registro não contém saídas de terminal, identificadores pessoais ou segredos.
