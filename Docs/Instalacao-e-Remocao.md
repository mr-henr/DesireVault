# Instalação e remoção

## Requisitos

- CachyOS ou outra distribuição Arch Linux, arquitetura x86_64.
- GTK 3, WebKitGTK 4.1 e `curl` disponíveis no sistema.
- Uma sessão gráfica KDE Plasma ou compatível para usar o assistente visual.

Rust, Cargo e os arquivos-fonte não são necessários para a instalação normal.

## Instalar pelo arquivo único

1. Abra `DesireVault-Installer.run` com a opção **Executar** no KDE.
2. Na tela de boas-vindas, avance para **Opções**.
3. Opcionalmente marque **Criar atalho na área de trabalho**.
4. Opcionalmente escolha **Procurar backup…** para selecionar um backup JSON.
5. Clique em **Instalar** e acompanhe a barra de progresso.
6. Abra **Desire Vault** pelo menu de aplicativos. Se um backup foi selecionado, ele será restaurado na primeira abertura.

A instalação é por usuário e não pede senha de administrador. Os arquivos do app ficam em `~/.local/bin`, `~/.local/share/applications` e `~/.local/share/icons` (ou nos diretórios definidos por XDG).

## Remover

No app, abra **Configurações** na barra lateral e escolha **Desinstalar aplicativo…**. A confirmação remove o executável, o lançador, o ícone e o atalho opcional da área de trabalho. A biblioteca é preservada por padrão. Marque **Apagar também minha biblioteca, capas e dados locais** apenas se quiser apagar também banco de jogos, cache e preferências.

Também é possível desinstalar pelo assistente: execute novamente `DesireVault-Installer.run`. Se o executável e o lançador estiverem instalados, a tela inicial mostra **Desinstalar Desire Vault…** e bloqueia uma instalação duplicada. Se a instalação não existir, essa opção fica oculta.

O qBittorrent é um programa separado e não é removido pelo Desire Vault.
