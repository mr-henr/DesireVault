# Biblioteca e backup

A biblioteca é local e utiliza SQLite. Cada jogo pode guardar título, status, prioridade, gêneros, ano, compatibilidade Proton, AppID Steam, desenvolvedora, distribuidora, descrição, capa, favorito e link Magnet/torrent.

## Exportar

Na barra lateral do app, abra **Backup e restauração** e escolha exportar. O sistema abre um seletor para definir o nome e o local do JSON; o app informa o caminho escolhido. As capas são incluídas no backup.

## Restaurar no app

Selecione o JSON e confirme a substituição. O app valida o formato e guarda uma cópia da biblioteca atual antes de importar. A restauração preserva IDs e ocorre em transação SQLite.

## Restaurar durante a instalação

Na etapa **Opções** do instalador, use **Procurar backup…**. O arquivo fica validado antes de ser preparado para a primeira abertura do app. Dados existentes não são substituídos sem essa escolha.

Os backups não incluem credenciais nem configurações do qBittorrent. O backup direto ao Google Drive não está implementado; o projeto não inclui credenciais OAuth compartilhadas.
