Desire Vault

No CachyOS/Arch, a forma mais simples é executar o arquivo único DesireVault-Installer.run, que embute o app e o assistente gráfico. No pacote de projeto extraído, executar instalar.sh também abre o assistente gráfico do Desire Vault. Ele exibe as etapas e o progresso, instala para sua conta sem senha de administrador e cria o atalho no menu. Sem ambiente gráfico, o mesmo script usa modo de terminal.

Backup: na barra lateral, abra “Backup e restauração” e escolha “Exportar backup”. Uma janela nativa permite escolher onde salvar; depois, o app mostra o caminho completo. O JSON inclui jogos, metadados e capas. Para restaurar, selecione o arquivo exportado e confirme a substituição. Antes de restaurar, o app salva automaticamente a biblioteca atual em seu diretório local de dados, com acesso restrito ao usuário.

A restauração preserva os IDs dos jogos para manter os vínculos de torrent e aplica as alterações em uma transação. Uma falha durante a importação não deixa a biblioteca parcialmente restaurada.

O instalador gráfico oferece a opção de selecionar um JSON e restaurá-lo automaticamente na primeira abertura após a instalação. O recurso é opcional; sem essa seleção, os dados existentes são preservados.

Os dados locais permanecem no perfil do aplicativo. O backup não inclui credenciais nem configurações do qBittorrent. Backup direto ao Google Drive ainda depende da configuração OAuth do aplicativo e não está incluído nesta versão.
