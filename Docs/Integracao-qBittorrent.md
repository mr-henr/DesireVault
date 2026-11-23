# Integração com qBittorrent

O Desire Vault pode preparar e iniciar o qBittorrent em segundo plano, ativar sua Web UI local com uma senha aleatória e conectar à API local. A interface do qBittorrent não precisa ficar aberta na frente do usuário.

A integração acompanha a fila: jogos ligados a torrents passam a **Baixando** durante a transferência e a **Baixado** quando concluída. A área de downloads também mostra atividade de upload/semeamento e permite controlar ou remover torrents. Remover uma tarefa pode manter ou excluir os arquivos conforme a escolha feita na confirmação.

A API fica vinculada à máquina local. O usuário pode abrir a janela do qBittorrent pelo app quando precisar. O qBittorrent continua sendo um componente independente e não é apagado ao desinstalar o Desire Vault.
