# Implementação 05 — Empacotamento CachyOS/Arch, AppImage, Autostart, Updater e Release Reproduzível

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Itens de Escopo Cobertos:** `I-14`, `I-11` (`release.yml`)
> **Critérios de Aceite Cobertos:** `CA-35`, `CA-36`, `CA-37`, `CA-38`, `CA-39`, `CA-42`

---

## 1. Contexto, Objetivo e Escopo

Esta implementação finaliza a Fase 17 tornando o **MaxTerminal instalável, autocontido, reproduzível e integrado nativamente ao CachyOS / Arch Linux**, além de fornecer pacotes portáteis AppImage e `.deb`:
- **Empacotamento Arch/CachyOS (`CA-35`, `CA-36`):** `packaging/arch/PKGBUILD` (build a partir do código-fonte em chroot limpo sem a feature `updater`), `packaging/arch/PKGBUILD-bin`, `.SRCINFO`, `maxterminal.install`, arquivo `packaging/linux/maxterminal.desktop` validado com `MimeType=x-scheme-handler/maxterminal;` e ícones `hicolor` (`16x16` a `512x512` PNG + `scalable/apps/maxterminal.svg`).
- **Desacoplamento de Bibliotecas Locais `@maxvue/*` e Recursos Offline (`CA-37`, `CA-42`):** Manifesto `release-manifest.json` e script `scripts/prepare-release-vendors.mjs` que congela `@maxvue/max-components-ui`, `@maxvue/max-use` e `@maxvue/max-pinia` em tarballs verificados por SHA-256 (eliminando qualquer symlink para `~/GitHub`), além de empacotar hooks de shell (`bash`/`zsh`/`fish`), ícones Iconify e temas Tabby no diretório `resources` do Tauri para execução 100% offline.
- **Sanitização de Ambiente para AppImage (`CA-37`):** Adaptador na `EnvPolicy` da Fase 02 (`src-tauri/crates/mt-pty/src/env_policy.rs`) que restaura as variáveis originais do sistema ou remove cirurgicamente os prefixos injetados pelo `AppDir` (`LD_LIBRARY_PATH`, `GIO_MODULE_DIR`, `GTK_PATH`, `QT_PLUGIN_PATH`, `PERLLIB`, `GSETTINGS_SCHEMA_DIR`) ao spawnar subprocessos no PTY.
- **Autostart (`--start-hidden`) e Updater Opcional Assinado (`CA-38`, `CA-39`):** Validar o `AutostartSync` entregue pela Fase 09: `Exec=<executável estável escapado> --start-hidden` quando `start_minimized`, via `os_get_autostart`/`os_set_autostart`, restaurando abas `"Adicionar ao Workspace"` pela Fase 07. `tauri-plugin-updater` fica isolado atrás da Cargo feature `updater` (desligada no pacote Arch/pacman; habilitada no AppImage com verificação de assinatura `minisign` e confirmação explícita do usuário).
- **Pipeline de Release Reproduzível (`CA-42`):** Script `scripts/verify-reproducible-build.sh` e workflow `.github/workflows/release.yml` executando duas builds independentes com `SOURCE_DATE_EPOCH` e `--remap-path-prefix`, comparando os hashes SHA-256 dos payloads não assinados e gerando SBOM CycloneDX/SPDX.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Criar em `packaging/`
- `packaging/arch/PKGBUILD` — Receita oficial Arch/CachyOS compilando a partir de fontes fixadas por commit/sha256, `cargo build --frozen --release --no-default-features` (sem `updater`), instalando binário, resources, `.desktop`, ícones e licença.
- `packaging/arch/PKGBUILD-bin` — Receita binária com `provides=('maxterminal')` e `conflicts=('maxterminal')`.
- `packaging/arch/.SRCINFO` — Metadados gerados a partir do `PKGBUILD`.
- `packaging/arch/maxterminal.install` — Mensagem pós-instalação orientativa (sem tocar em `/home`, sem alterar associação MIME global do usuário e sem criar autostart forçado).
- `packaging/linux/maxterminal.desktop` — Entrada XDG Desktop Entry em `pt-BR` com `Exec=maxterminal %u` e `MimeType=x-scheme-handler/maxterminal;`.
- `packaging/linux/icons/hicolor/{16x16,32x32,48x48,64x64,128x128,256x256,512x512}/apps/maxterminal.png` e `packaging/linux/icons/hicolor/scalable/apps/maxterminal.svg`.

### 2.2. Criar ou Modificar em `src-tauri/` e `src/`
- `src-tauri/Cargo.toml` — Definir `[features] default = []`, `updater = ["dep:tauri-plugin-updater"]`; consumir a integração de autostart já registrada pela Fase 09.
- `src-tauri/tauri.conf.json` — Configurar `bundle` (`targets: ["appimage", "deb"]`, `resources`, `icon`, `linux.deb.depends`, `linux.appimage`) e endpoint/pubkey `minisign` do `updater`.
- `src-tauri/crates/mt-pty/src/env_policy.rs` — Implementar `AppImageEnvCleaner` dentro da `EnvPolicy` para limpar variáveis injetadas por `APPDIR` antes de criar processos filhos no PTY.
- `src-tauri/crates/mt-os/src/autostart.rs` e `src-tauri/src/commands/os.rs` — Contratos existentes da Fase 09, somente consumidos em testes de pacote: `AutostartSync`, `AutostartStatus`, `os_get_autostart` e `os_set_autostart(enabled: bool)`. Nenhum módulo/comando/parser paralelo é criado.
- `src-tauri/src/updater.rs` e `src-tauri/src/commands/updater.rs` — Comandos compilados sob `#[cfg(feature = "updater")]` (`app_update_check`, `app_update_install_confirmed`) exigindo confirmação explícita do usuário antes de baixar/aplicar atualização e retornando `{ supported: false, reason: "managed_by_system_package_manager" }` quando compilado sem a feature `updater`.

### 2.3. Criar em `scripts/` e `.github/workflows/`
- `release-manifest.json` — Manifesto imutável registrando versão, `SOURCE_DATE_EPOCH`, commits e hashes SHA-256 dos tarballs de `@maxvue/max-components-ui`, `@maxvue/max-use` e `@maxvue/max-pinia`, versões de Rust/Node/WebKitGTK e ferramentas de bundle.
- `scripts/prepare-release-vendors.mjs` — Empacota ou valida os tarballs `vendor/maxvue-*.tgz` contra os hashes SHA-256 de `release-manifest.json` para que o build em chroot limpo ou container sem `~/GitHub` funcione 100% offline.
- `scripts/verify-reproducible-build.sh` — Executa duas compilações limpas em diretórios distintos (`/tmp/mt-repro-a` e `/tmp/mt-repro-b`) com `SOURCE_DATE_EPOCH`, `RUSTFLAGS="--remap-path-prefix=..."` e `LC_ALL=C.UTF-8`, comparando bit a bit os binários ELF e resources produzidos.
- `.github/workflows/release.yml` — Pipeline protegido de release que valida reprodutibilidade, gera `.pkg.tar.zst`, `.AppImage`, `.deb`, SBOM e assinaturas `minisign`.

---

## 3. Especificação Técnica Exaustiva

### 3.1. Limpeza de Variáveis AppImage no PTY (`mt-pty/src/env_policy.rs`)

Quando o MaxTerminal é executado via AppImage, o runtime `AppRun` define `APPDIR=/tmp/.mount_maxterXXXXXX` e injeta caminhos de bibliotecas empacotadas em `LD_LIBRARY_PATH`, `GIO_MODULE_DIR`, etc. Se um comando do usuário no terminal (`git`, `python`, `cargo`, `php`) herdar essas variáveis, ele falha com erros de símbolos GLIBC/OpenSSL/GTK incompatíveis.

```rust
pub struct AppImageEnvSnapshot {
    pub appdir: String,
    pub original_vars: std::collections::BTreeMap<String, Option<String>>,
}

impl AppImageEnvSnapshot {
    /// Capturado no início do `main()` e aplicado pelo `EnvPolicy` ao spawnar cada PTY.
    pub fn sanitize_child_env(
        env: &mut std::collections::BTreeMap<String, String>,
        appdir: Option<&str>,
    ) {
        let Some(mount_prefix) = appdir.filter(|p| !p.is_empty()) else {
            return;
        };
        const PATH_LIKE_VARS: &[&str] = &[
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "GIO_MODULE_DIR",
            "GTK_PATH",
            "GTK_EXE_PREFIX",
            "GTK_DATA_PREFIX",
            "GSETTINGS_SCHEMA_DIR",
            "QT_PLUGIN_PATH",
            "XDG_DATA_DIRS",
            "PERLLIB",
            "PYTHONHOME",
            "PYTHONPATH",
        ];
        for key in PATH_LIKE_VARS {
            if let Some(val) = env.get(*key).cloned() {
                let filtered: Vec<&str> = val
                    .split(':')
                    .filter(|segment| !segment.is_empty() && !segment.starts_with(mount_prefix))
                    .collect();
                if filtered.is_empty() {
                    env.remove(*key);
                } else {
                    env.insert((*key).to_owned(), filtered.join(":"));
                }
            }
        }
        for appimage_key in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
            env.remove(appimage_key);
        }
    }
}
```

### 3.2. `packaging/linux/maxterminal.desktop` e `packaging/arch/PKGBUILD`

Arquivo `packaging/linux/maxterminal.desktop`:
```ini
[Desktop Entry]
Type=Application
Version=1.0
Name=MaxTerminal
GenericName=Emulador de Terminal e Orquestrador de Agentes de IA
Comment=Terminal acelerado por GPU integrado ao ecossistema Max
Exec=maxterminal %u
Icon=maxterminal
Terminal=false
Categories=System;TerminalEmulator;Development;
MimeType=x-scheme-handler/maxterminal;
Keywords=terminal;shell;prompt;command;ai;maxcode;
StartupNotify=true
StartupWMClass=maxterminal
```

Estrutura essencial de `packaging/arch/PKGBUILD`:
```bash
# Maintainer: MaxTerminal Team <maintainers@maxterminal.local>
pkgname=maxterminal
pkgver=0.1.0
pkgrel=1
pkgdesc='Emulador de terminal acelerado por GPU e orquestrador de Agentes de IA integrado ao ecossistema Max'
arch=('x86_64')
url='https://github.com/johnattas/MaxTerminal'
license=('LicenseRef-MaxProprietary')
depends=(
    'webkit2gtk-4.1'
    'gtk3'
    'libsoup3'
    'libappindicator-gtk3'
    'librsvg'
    'openssl'
    'sqlite'
    'gcc-libs'
    'glibc'
    'hicolor-icon-theme'
)
makedepends=(
    'cargo'
    'nodejs'
    'npm'
    'pkgconf'
    'base-devel'
)
optdepends=(
    'github-cli: integração nativa com a aba fixa de GitHub Issues'
    'xdotool: suporte adicional de foco em sessões X11'
    'zsh: suporte a shell Zsh com integração OSC 133'
    'fish: suporte a shell Fish com integração OSC 133'
)
options=('!lto')
install=maxterminal.install
source=(
    "maxterminal-${pkgver}.tar.gz"
)
sha256sums=('SKIP') # substituído pelo hash SHA-256 exato gerado pelo release-manifest.json

prepare() {
    cd "${srcdir}/${pkgname}-${pkgver}"
    node scripts/prepare-release-vendors.mjs --verify-offline
    npm ci --offline --no-audit --no-fund
    cargo fetch --locked --manifest-path src-tauri/Cargo.toml
}

build() {
    cd "${srcdir}/${pkgname}-${pkgver}"
    export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1761782400}"
    export LC_ALL=C.UTF-8
    export RUSTFLAGS="${RUSTFLAGS} --remap-path-prefix=${srcdir}=/usr/src/debug/${pkgname}"
    npm run build
    cargo build --manifest-path src-tauri/Cargo.toml --frozen --release --no-default-features
}

check() {
    cd "${srcdir}/${pkgname}-${pkgver}"
    cargo test --manifest-path src-tauri/Cargo.toml --frozen --release --no-default-features -p mt-db --test search_query
}

package() {
    cd "${srcdir}/${pkgname}-${pkgver}"
    install -Dm755 "src-tauri/target/release/maxterminal" "${pkgdir}/usr/bin/maxterminal"
    install -Dm644 "packaging/linux/maxterminal.desktop" "${pkgdir}/usr/share/applications/maxterminal.desktop"

    for size in 16 32 48 64 128 256 512; do
        install -Dm644 \
            "packaging/linux/icons/hicolor/${size}x${size}/apps/maxterminal.png" \
            "${pkgdir}/usr/share/icons/hicolor/${size}x${size}/apps/maxterminal.png"
    done
    install -Dm644 \
        "packaging/linux/icons/hicolor/scalable/apps/maxterminal.svg" \
        "${pkgdir}/usr/share/icons/hicolor/scalable/apps/maxterminal.svg"

    install -d "${pkgdir}/usr/lib/maxterminal/resources"
    cp -r src-tauri/resources/* "${pkgdir}/usr/lib/maxterminal/resources/"
}
```

### 3.3. Autostart (`--start-hidden`) e Updater Condicional (`CA-38`, `CA-39`)

O contrato pertence integralmente à Fase 09: consumir `AutostartSync`, `AutostartStatus`, `os_get_autostart` e `os_set_autostart`, sem redefinir DTOs nem instalar outro plugin. Validar no pacote instalado:
1. Ativar/desativar cria/remove a entrada em `$XDG_CONFIG_HOME/autostart` (fallback `~/.config/autostart`), por comando Rust validado; permissões JS diretas de autostart permanecem negadas.
2. `start_minimized = true` inclui `--start-hidden`; `false` remove a flag. O parser e `os_ui_ready` da 09 preservam a intenção inicial. Sem bandeja disponível, o fallback torna a janela acessível na taskbar.
3. `Exec` usa executável instalado estável (ou caminho do arquivo AppImage, nunca montagem temporária). Alterar/mover o AppImage exige reconciliação pela 09.
4. A Fase 07 restaura abas `add_to_workspace`; remoção manual do `.desktop` e falha de persistência exercitam reconciliação/compensação da 09, sem recriar regras nesta fase.

```rust
// src-tauri/src/updater.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapabilityDto {
    pub compilable_feature_enabled: bool,
    pub runtime_supported: bool,
    pub reason_pt_br: Option<String>,
}
```
- Quando compilado para Arch Linux (`--no-default-features`, sem `updater`), `compilable_feature_enabled` é `false` e nenhuma dependência de auto-atualização é linkada no binário (`CA-39`).
- Quando compilado para AppImage (`--features updater`), verifica a assinatura `minisign` do artefato contra a chave pública embutida em `tauri.conf.json` e exige chamada explícita `app_update_install_confirmed(version)` acionada pelo botão de confirmação do usuário na UI.

---

## 4. Regras de Negócio e Casos de Borda

- **Proibição de tocar no `$HOME` durante `package()` ou `post_install` (`CA-36`):** O pacote `.pkg.tar.zst` instala arquivos exclusivamente sob `/usr/`. Preferência de `xdg-mime default maxterminal.desktop x-scheme-handler/maxterminal` e `~/.config/autostart/maxterminal.desktop` são ações em nível de usuário, nunca do script root do `pacman`.
- **Reprodutibilidade Honesta (`CA-42`):** `scripts/verify-reproducible-build.sh` compara os hashes SHA-256 dos binários ELF e dos arquivos de recursos entre duas builds limpas em caminhos diferentes; qualquer divergência faz o script falhar com relatório `diffoscope`/`cmp` detalhado.
