# Desire Vault

Biblioteca desktop local para organizar jogos, capas e metadados em CachyOS/Linux.

## Instalação para usuários

Use `DesireVault-Installer.run`, incluído na raiz do ZIP. O assistente instala para o usuário atual, oferece restauração de backup, permite escolher um atalho na área de trabalho e disponibiliza desinstalação. Não exige Rust, Cargo nem senha de administrador.

## Documentação

Consulte a pasta [`Docs/`](Docs/README.md) para instruções de instalação e remoção, biblioteca e backup, integração com qBittorrent e arquitetura/desenvolvimento.

## Construir

Requisitos: Rust/Cargo, GTK 3, WebKitGTK 4.1, curl e as dependências Linux do Tauri 2.

```sh
cargo build --release --offline --bin game-library-manager
cargo build --release --offline --bin desire-vault-installer
```

O executável do app fica em `target/release/game-library-manager`. O assistente GTK fica em `target/release/desire-vault-installer` e inclui o app e o ícone de `outputs/GameLibraryManager/` no build.

## Dados e integrações

A biblioteca usa SQLite local. Os backups JSON incluem jogos, metadados e capas; credenciais do qBittorrent não são incluídas. Em **Configurações**, o usuário pode desinstalar o app e escolher se mantém ou apaga seus dados. A integração direta com Google Drive ainda não existe, pois requer configuração OAuth própria do aplicativo.
