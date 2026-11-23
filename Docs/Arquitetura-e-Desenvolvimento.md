# Arquitetura e desenvolvimento

- **Interface:** HTML, CSS e JavaScript em `src/`.
- **Aplicativo desktop:** Tauri 2 e Rust em `src-tauri/src/main.rs`.
- **Persistência:** SQLite local; comandos Tauri ligam a interface ao banco e às integrações do sistema.
- **Assistente gráfico:** GTK 3 e Rust em `src-tauri/src/installer.rs`.
- **Marca:** SVGs em `src/assets/` e ícone em `icons/`.
- **Entrega:** `outputs/GameLibraryManager/` contém os executáveis usados pelo pacote; o `.run` raiz embute o assistente, app e ícone.

## Compilar

Em CachyOS/Arch, com Rust/Cargo e dependências de compilação instaladas:

```sh
cargo build --release --offline --bin game-library-manager
cargo build --release --offline --bin desire-vault-installer
```

O executável do app fica em `target/release/game-library-manager`; o assistente, em `target/release/desire-vault-installer`. Para gerar o `.run` atualizado, o binário do app e o ícone devem primeiro ser copiados para `outputs/GameLibraryManager/` e então o assistente precisa ser recompilado, pois ele os inclui no build.

O projeto tem build offline com dependências travadas em `Cargo.lock`. O frontend é distribuído localmente e não depende de serviço web para abrir.
