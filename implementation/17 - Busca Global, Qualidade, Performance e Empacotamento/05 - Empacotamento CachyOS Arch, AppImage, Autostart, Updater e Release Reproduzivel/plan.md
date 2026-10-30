# Plano de Execução — Implementação 05: Empacotamento CachyOS/Arch, AppImage, Autostart, Updater e Release Reproduzível

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Especificação de Referência:** [`spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Desacoplamento de `@maxvue/*` e Manifesto de Recursos Offline
1. Criar `release-manifest.json` e `scripts/prepare-release-vendors.mjs` para empacotar `@maxvue/max-components-ui`, `@maxvue/max-use` e `@maxvue/max-pinia` em tarballs imutáveis com verificação de SHA-256, garantindo que o build de produção não dependa de symlinks para `~/GitHub`.
2. Configurar o diretório `src-tauri/resources/` com os scripts de hook de shell (`bash`, `zsh`, `fish`), temas padrão e assets necessários para execução 100% offline, resolvendo os caminhos via `AppPaths` / `PathResolver` do Tauri.

### Passo 2: Limpeza de Ambiente AppImage na `EnvPolicy`, Autostart e Updater Condicional
1. Atualizar `src-tauri/crates/mt-pty/src/env_policy.rs` adicionando `AppImageEnvSnapshot::sanitize_child_env` para remover segmentos do `APPDIR` de `LD_LIBRARY_PATH`, `GIO_MODULE_DIR`, `GTK_PATH`, etc., preservando as variáveis legítimas do usuário.
2. Criar `src-tauri/src/autostart.rs` e `src-tauri/src/commands/autostart.rs` integrando `tauri-plugin-autostart` com a flag `--minimized`, iniciando oculto na bandeja e reabrindo as abas configuradas com `"Adicionar ao Workspace"`.
3. Configurar a feature opcional `updater` em `src-tauri/Cargo.toml` e criar `src-tauri/src/updater.rs` + `src-tauri/src/commands/updater.rs` garantindo verificação de assinatura `minisign` e exigência de confirmação explícita do usuário.

### Passo 3: Arquivos Desktop, Ícones Hicolor e Receitas `PKGBUILD` / `PKGBUILD-bin`
1. Criar `packaging/linux/maxterminal.desktop` e os ícones em `packaging/linux/icons/hicolor/{16x16,32x32,48x48,64x64,128x128,256x256,512x512,scalable}/apps/`.
2. Criar `packaging/arch/PKGBUILD`, `packaging/arch/PKGBUILD-bin`, `packaging/arch/.SRCINFO` e `packaging/arch/maxterminal.install`.
3. Configurar os alvos `appimage` e `deb` em `src-tauri/tauri.conf.json`.

### Passo 4: Verificação de Build Reproduzível e Workflow de Release
1. Criar `scripts/verify-reproducible-build.sh` executando duas builds limpas em diretórios separados com `SOURCE_DATE_EPOCH` e `--remap-path-prefix`, comparando os hashes SHA-256 dos payloads não assinados.
2. Criar `.github/workflows/release.yml` automatizando o build reproduzível, geração de SBOM, empacotamento Arch/AppImage/deb e assinatura `minisign`.

---

## 2. Comandos Exatos de Verificação e Testes

Executar a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`:

```bash
# 1. Validar arquivo .desktop e presença dos ícones hicolor (CA-36)
desktop-file-validate packaging/linux/maxterminal.desktop

# 2. Testar compilação sem a feature updater (modo Arch/pacman) e com a feature updater (modo AppImage) (CA-39)
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
cargo check --manifest-path src-tauri/Cargo.toml --features updater

# 3. Executar testes unitários da sanitização de ambiente AppImage e autostart (CA-37, CA-38)
cargo nextest run --manifest-path src-tauri/Cargo.toml -p mt-pty env_policy
cargo nextest run --manifest-path src-tauri/Cargo.toml -p maxterminal autostart

# 4. Validar PKGBUILD e pacote gerado com makepkg e namcap (CA-35)
namcap packaging/arch/PKGBUILD
(cd packaging/arch && makepkg -cf --nodeps && namcap maxterminal-*.pkg.tar.zst)

# 5. Gerar bundles AppImage e .deb (CA-37)
npm run tauri -- build --bundles appimage,deb -- --features updater

# 6. Verificar reprodutibilidade de duas builds limpas (CA-42)
bash scripts/verify-reproducible-build.sh
```

---

## 3. Mapeamento para os Critérios de Aceite da Fase 17

| Critério | Teste / Verificação Responsável | Resultado Esperado |
|----------|---------------------------------|--------------------|
| `CA-35` | `makepkg` em chroot limpo + `namcap PKGBUILD` + `namcap *.pkg.tar.zst` | Gera `maxterminal-<ver>-1-x86_64.pkg.tar.zst` sem erros (`E:`) no `namcap`. |
| `CA-36` | `desktop-file-validate` + teste de invocação `maxterminal://` | `.desktop` válido; ícones presentes em `/usr/share/icons/hicolor/{16x16..512x512,scalable}/apps/`; URI válida abre/foca o app e inválida é rejeitada sem alterar MIME global no post-install. |
| `CA-37` | `npm run tauri -- build --bundles appimage,deb` + teste de `EnvPolicy` | Gera AppImage e `.deb` autocontidos sem depender de `~/GitHub` ou Node; FUSE e `--appimage-extract` funcionam offline e subprocessos não herdam `LD_LIBRARY_PATH` do `AppDir`. |
| `CA-38` | Testes de `autostart` + smoke de inicialização `--minimized` | Ativar/desativar na bandeja cria/remove `~/.config/autostart/maxterminal.desktop` com `--minimized`; inicia oculto na bandeja reabrindo as abas `"Adicionar ao Workspace"`. |
| `CA-39` | Build `--no-default-features` vs `--features updater` | Pacote Arch compila sem `updater`; AppImage com `updater` valida assinatura `minisign` e exige confirmação explícita do usuário. |
| `CA-42` | `bash scripts/verify-reproducible-build.sh` + `release-manifest.json` | Duas builds limpas do mesmo manifesto reproduzem exatamente os hashes SHA-256 dos payloads não assinados. |
