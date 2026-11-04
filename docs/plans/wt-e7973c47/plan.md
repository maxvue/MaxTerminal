# Plano — gestão de temas e MaxApp

## Objetivo
Concluir a gestão de temas em Configurações Gerais → Aparência & Temas e substituir o uso de AppShell pelo MaxApp em `blank-mode`, preservando a integração da paleta de cores.

**Base encontrada:** criação, edição, duplicação e exclusão já possuem componentes, store e IPC. `App.vue` já utiliza `MaxApp blank-mode no-login controlled-theme :colors="maxAppColors"`, mas mantém AppShell dentro do slot `#blank`. O código local de MaxComponentsUi confirma esse slot, a prop `colors: PartialColorPalette` e sua atualização reativa. Temas nativos são somente leitura, inclusive no backend; personalizações devem ocorrer por duplicação.

## Arquivos
- `src/app/App.vue` — retirar o uso de AppShell e preservar o enquadramento do workspace.
- `src/components/preferences/appearance/ThemeSwatch.vue` — explicitar as ações disponíveis por tema.
- `src/components/preferences/appearance/AppearancePreferences.vue` — completar os fluxos e estados das ações existentes.
- `src/components/preferences/appearance/ThemeEditorDialog.vue` e `src/stores/themeEditor.ts` — ajustar o editor somente onde necessário.
- `src/stores/theme.ts` e `src/theme/maxAppPalette.ts` — ajustar atualização do catálogo e paleta somente se necessário.
- Criar testes focados em `src/components/preferences/__tests__/AppearanceThemeManagement.spec.ts` e `src/components/preferences/__tests__/MaxAppIntegration.spec.ts`.

## Passos
1. Reaproveitar o catálogo e o editor existentes. Tornar claras as ações Novo Tema, Editar, Duplicar e Excluir, com componentes MaxComponentsUi, rótulos acessíveis e eventos que não ativem o cartão acidentalmente. Para temas nativos, manter edição/exclusão bloqueadas e orientar a duplicação.
2. Completar os fluxos de criação e edição com validação de nome e cores, proteção contra submissão repetida e feedback de erro. Reutilizar MaxPinia, MaxUse e o adapter IPC existentes, sem criar persistência paralela nem alterar contratos Rust/Tauri.
3. Garantir atualização do catálogo após salvar/duplicar/excluir, preservação do original na duplicação, confirmação de exclusão com impactos e fallback já definido para tema ativo e referências por projeto. Preservar cancelamento de pré-visualização sem persistir mudanças não confirmadas.
4. Remover AppShell do template/import de `App.vue` e renderizar o workspace diretamente no slot `#blank` do MaxApp. Transferir apenas as restrições necessárias de altura, flex e overflow; preservar estados de inicialização/erro/recuperação, modais, atalhos, rota nomeada e configurações offline. Manter um único host global de toast/confirmação, fornecido pelo MaxApp.
5. Manter `colors` derivado de `effectiveResolved` via `resolveMaxAppColors`, com bootstrap/fallback e `controlled-theme`. Validar tema claro/escuro, troca, pré-visualização, cancelamento e override por projeto, evitando disputa entre a paleta MaxApp e os tokens aplicados ao terminal.
6. Adicionar testes dos fluxos e da composição MaxApp sem AppShell; executar testes focados, typechecks e lint na implementação. Fazer verificação visual de dimensionamento do terminal e acessibilidade das ações.

## Aceite
- Criar, editar, duplicar e excluir temas personalizados funciona e persiste após reabrir; temas nativos permanecem protegidos e duplicáveis.
- Exclusão exige confirmação, informa impactos e não deixa referências inválidas; falhas são exibidas sem falso sucesso.
- AppShell não é mais usado em `App.vue`; MaxApp em `blank-mode` mantém o workspace completo, sem login remoto, barras extras ou hosts duplicados.
- Paleta da interface e cores do terminal acompanham o tema efetivo, inclusive preview/cancelamento e reinicialização, sem regressões de layout.
- Testes focados, `npm run typecheck`, `npm run typecheck:vue`, `npm run lint` e `npm run lint:style` aprovados na implementação.
