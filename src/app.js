const invoke = window.__TAURI__?.core?.invoke;
const $ = (id) => document.getElementById(id);
const grid = $('gameGrid');
const dialog = $('gameDialog');
const detailDialog = $('detailDialog');
let games = [];
let section = 'Todos os jogos';
let favoritesOnly = false;
let currentDetail = null;
let pendingRemoval = null;
let pendingTorrentGameId = null;
let pendingQbitRemoval = null;
let pendingRestoreBackup = null;
let qbitRefreshTimer = null;
let qbitActivityTimer = null;
let qbitActivityBusy = false;
let qbitRefreshBusy = false;
let qbitStartupPromise = null;
let qbitStartupError = '';
let qbitStartupDone = false;
let startupLoading = true;
const startupLoadingAt = performance.now();
let lastQuery = '';
let toastTimer;

function notify(message) {
  const toast = $('toast'); toast.textContent = message; toast.classList.remove('hidden');
  clearTimeout(toastTimer); toastTimer = setTimeout(() => toast.classList.add('hidden'), 2600);
}
function setQbitStatus(message, state = 'idle') {
  const status = $('qbitStatus');
  status.className = `qbit-status${state === 'loading' ? ' is-loading' : state === 'error' ? ' is-error' : state === 'success' ? ' is-success' : ''}`;
  status.replaceChildren();
  if (state === 'loading') {
    const spinner = document.createElement('span'); spinner.className = 'loading-spinner'; spinner.setAttribute('aria-hidden', 'true'); status.append(spinner);
  }
  const text = document.createElement('span'); text.textContent = message; status.append(text);
  $('qbitNavSpinner')?.classList.toggle('hidden', state !== 'loading');
  $('qbitNav').title = message;
  const activity = $('backgroundStatus');
  if (activity) {
    activity.className = `background-status${state === 'loading' ? ' is-loading' : state === 'error' ? ' is-error' : state === 'success' ? ' is-success' : ''}`;
    activity.replaceChildren();
    if (state === 'loading') { const spinner = document.createElement('span'); spinner.className = 'loading-spinner'; spinner.setAttribute('aria-hidden', 'true'); activity.append(spinner); }
    else { const dot = document.createElement('i'); dot.setAttribute('aria-hidden', 'true'); activity.append(dot); }
    const label = document.createElement('span'); label.textContent = state === 'loading' ? 'Preparando downloads…' : state === 'error' ? 'Downloads precisam de atenção' : state === 'success' ? 'Downloads prontos' : 'Downloads'; activity.append(label);
  }
}
function setButtonLoading(button, loading, label = '') {
  if (loading) {
    button.dataset.idleLabel ??= button.textContent.trim();
    button.disabled = true; button.classList.add('is-loading'); button.setAttribute('aria-busy', 'true');
    button.replaceChildren();
    const spinner = document.createElement('span'); spinner.className = 'loading-spinner'; spinner.setAttribute('aria-hidden', 'true');
    button.append(spinner, document.createTextNode(label));
  } else {
    button.disabled = false; button.classList.remove('is-loading'); button.removeAttribute('aria-busy');
    if (button.dataset.idleLabel) button.textContent = button.dataset.idleLabel;
  }
}
function finishStartupLoading() {
  if (!startupLoading) return;
  startupLoading = false;
  const delay = Math.max(0, 850 - (performance.now() - startupLoadingAt));
  setTimeout(() => {
    const loading = $('appLoading');
    if (loading) {
      loading.setAttribute('aria-busy', 'false');
      loading.classList.add('hidden');
    }
  }, delay);
}
function beginQbitStartup() {
  qbitStartupError = ''; qbitStartupDone = false;
  setQbitStatus('Preparando os downloads em segundo plano…', 'loading');
  qbitStartupPromise = invoke('ensure_qbittorrent_ready').then(version => {
    qbitStartupDone = true;
    setQbitStatus(`Downloads prontos · qBittorrent ${version}.`, 'success');
    void refreshQbitActivity();
    if (!qbitActivityTimer) qbitActivityTimer = setInterval(refreshQbitActivity, 20000);
    return version;
  }).catch(error => {
    qbitStartupDone = true; qbitStartupError = String(error);
    updateQbitActivity([]);
    setQbitStatus(qbitStartupError, 'error');
    return null;
  });
  return qbitStartupPromise;
}
function parseCover(cover, title) {
  if (cover?.startsWith('data:image/')) return `<img src="${cover}" alt="Capa de ${escapeHtml(title)}" draggable="false">`;
  return '<div class="cover-empty"><span>▣</span><small>CAPA NÃO DISPONÍVEL</small></div>';
}
function escapeHtml(value='') { return String(value).replace(/[&<>"']/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
function currentFilters() {
  return { status: section === 'Todos os jogos' || section === 'Recentes' ? 'Todos' : section,
    query: $('searchInput').value.trim(), sort: $('sortFilter').value, favorites: favoritesOnly };
}
async function refresh() {
  if (!invoke) { notify('O aplicativo precisa ser aberto pelo atalho do Game Library Manager.'); finishStartupLoading(); return; }
  const filters = currentFilters();
  try {
    games = await invoke('list_games', filters);
    if ($('protonFilter').value !== 'Todos') games = games.filter(g => g.proton === $('protonFilter').value);
    render();
    await updateCounts();
  } catch (e) { notify(`Não foi possível carregar a biblioteca: ${e}`); }
  finally { finishStartupLoading(); }
}
function render() {
  $('sectionTitle').textContent = section.toLocaleUpperCase('pt-BR');
  $('shownCount').textContent = `${games.length} ${games.length === 1 ? 'jogo' : 'jogos'}`;
  $('footerCount').textContent = `${games.length} ${games.length === 1 ? 'jogo' : 'jogos'} na biblioteca`;
  grid.innerHTML = '';
  $('emptyState').classList.toggle('hidden', games.length > 0);
  grid.classList.toggle('hidden', games.length === 0);
  const makeCard = (game) => {
    const card = document.createElement('article'); card.className = 'game-card';
    card.innerHTML = `<div class="cover-art">${parseCover(game.cover,game.title)}<span class="cover-badge">${escapeHtml(game.proton === 'Sem dados' ? game.status : game.proton.toUpperCase())}</span><button class="cover-fav ${game.favorite ? 'is-favorite' : ''}" title="${game.favorite ? 'Remover dos favoritos' : 'Adicionar aos favoritos'}" aria-label="${game.favorite ? 'Remover dos favoritos' : 'Adicionar aos favoritos'}">${game.favorite ? '★' : '☆'}</button></div><div class="card-actions"><button class="card-menu-trigger" type="button" title="Mais opções" aria-label="Mais opções para ${escapeHtml(game.title)}" aria-expanded="false"><svg viewBox="0 0 4 16" aria-hidden="true"><circle cx="2" cy="2" r="1.35"/><circle cx="2" cy="8" r="1.35"/><circle cx="2" cy="14" r="1.35"/></svg></button><div class="card-menu hidden"><button type="button" data-action="edit">Editar jogo</button><button type="button" class="menu-remove" data-action="remove">${game.status === 'Na lista' ? 'Remover da lista de desejos' : 'Remover da biblioteca'}</button></div></div><div class="cover-title">${escapeHtml(game.title)}</div><div class="cover-subtitle">${escapeHtml(game.status)}${game.genre ? ` · ${escapeHtml(game.genre)}` : ''}</div>`;
    card.addEventListener('click', () => openDetails(game));
    card.querySelector('.cover-fav').addEventListener('click', async (e) => { e.stopPropagation(); await invoke('set_favorite', {id:game.id,value:!game.favorite}); await refresh(); });
    const menuTrigger=card.querySelector('.card-menu-trigger'), menu=card.querySelector('.card-menu');
    menuTrigger.addEventListener('click',(e)=>{e.stopPropagation();document.querySelectorAll('.card-menu:not(.hidden)').forEach(open=>{open.classList.add('hidden');open.previousElementSibling?.setAttribute('aria-expanded','false');});menu.classList.toggle('hidden');menuTrigger.setAttribute('aria-expanded',String(!menu.classList.contains('hidden')));});
    menu.addEventListener('click',async(e)=>{e.stopPropagation();const action=e.target.closest('[data-action]')?.dataset.action;menu.classList.add('hidden');menuTrigger.setAttribute('aria-expanded','false');if(action==='edit')openForm(game,true);if(action==='remove')askRemove(game);});
    return card;
  };
  const grouped = section === 'Todos os jogos' && !favoritesOnly && !$('searchInput').value.trim() && $('protonFilter').value === 'Todos';
  grid.classList.toggle('grouped', grouped);
  if (grouped) {
    const groups = [['Jogando agora', 'Jogando'], ['Lista de desejos', 'Na lista'], ['Baixando', 'Baixando'], ['Baixados', 'Baixado'], ['Concluídos', 'Concluído'], ['Abandonados', 'Abandonado'], ['Não funciona no Linux', 'Não funciona no Linux']];
    for (const [label, status] of groups) {
      const items = games.filter(g => g.status === status);
      if (!items.length) continue;
      const group = document.createElement('section'); group.className = 'library-group';
      group.innerHTML = `<div class="group-heading"><span class="section-chevron">⌄</span><span>${label.toLocaleUpperCase('pt-BR')}</span><small>${items.length}</small></div>`;
      const groupGrid = document.createElement('div'); groupGrid.className='group-grid';
      for (const game of items) groupGrid.append(makeCard(game));
      group.append(groupGrid); grid.append(group);
    }
  } else {
    grid.classList.remove('grouped');
    for (const game of games) grid.append(makeCard(game));
  }
}
async function updateCounts() {
  const all = await invoke('list_games', {status:'Todos',query:'',sort:'title',favorites:false});
  $('allCount').textContent = all.length;
  $('wishCount').textContent = all.filter(g => g.status === 'Na lista').length;
  $('downloadingCount').textContent = all.filter(g => g.status === 'Baixando').length;
  $('downloadedCount').textContent = all.filter(g => g.status === 'Baixado').length;
  $('favoriteCount').textContent = all.filter(g => g.favorite).length;
}
function selectSection(button) {
  document.querySelectorAll('#mainNav button').forEach(b => b.classList.toggle('active', b === button));
  section = button.dataset.section;
  favoritesOnly = false;
  if (section === 'Recentes') $('sortFilter').value = 'recent';
  refresh();
}
function updatePriorityVisibility(){ $('priorityField').classList.toggle('hidden',$('gameStatus').value!=='Na lista'); }
$('gameStatus').addEventListener('change',updatePriorityVisibility);
document.querySelectorAll('#mainNav [data-section]').forEach(button => button.addEventListener('click', () => selectSection(button)));
$('favoriteNav').addEventListener('click', () => { favoritesOnly = true; section = 'Favoritos'; document.querySelectorAll('#mainNav button').forEach(b=>b.classList.remove('active')); refresh(); });
$('bookmarkBtn').addEventListener('click', () => $('favoriteNav').click());
$('focusSearch').addEventListener('click', () => $('searchInput').focus());
let queryDelay;
$('searchInput').addEventListener('input', () => { clearTimeout(queryDelay); queryDelay = setTimeout(refresh, 130); });
$('filterBtn').addEventListener('click', () => $('filterPanel').classList.toggle('hidden'));
['statusFilter','protonFilter','sortFilter'].forEach(id => $(id).addEventListener('change', async () => { if(id==='statusFilter'){ section=$('statusFilter').value==='Todos'?'Todos os jogos':$('statusFilter').value; } await refresh(); }));
$('clearFilters').addEventListener('click', () => { $('statusFilter').value='Todos';$('protonFilter').value='Todos';$('sortFilter').value='title';$('searchInput').value=''; section='Todos os jogos';favoritesOnly=false;refresh(); });
$('customizeBtn').addEventListener('click', () => { document.body.classList.toggle('compact-covers'); notify(document.body.classList.contains('compact-covers')?'Visualização compacta ativada':'Visualização confortável ativada'); });
function setView(list){grid.classList.toggle('list-view',list);document.querySelectorAll('.view-toggle button').forEach(b=>b.classList.toggle('active',b.id===(list?'listViewBtn':'gridViewBtn')));}
$('gridViewBtn').addEventListener('click',()=>setView(false));
$('listViewBtn').addEventListener('click',()=>setView(true));
$('emptyAdd').addEventListener('click', () => openForm());
$('addBtn').addEventListener('click', () => openForm());
$('cancelBtn').addEventListener('click', () => dialog.close());
document.querySelectorAll('.close-dialog').forEach(button=>button.addEventListener('click',()=>button.closest('dialog')?.close()));
function openForm(game=null, manual=false) {
  $('gameForm').reset(); $('formError').textContent=''; $('coverInput').value='';
  const searching = !game && !manual;
  $('steamSearchPanel').classList.toggle('hidden',!searching);
  $('gameForm').classList.toggle('hidden',searching);
  $('formTitle').textContent=game?(game.id?'Editar jogo':'Confirmar importação da Steam'):'Adicionar jogo'; $('gameId').value=game?.id||0;
  $('deleteBtn').classList.toggle('hidden',!game);
  showCoverPreview(game?.cover || '', game?.appId ? 'Capa importada da Steam' : 'Capa salva localmente');
  if(game){ $('titleInput').value=game.title; $('gameStatus').value=game.status; $('gamePriority').value=game.priority||'Tenho interesse'; $('genreInput').value=game.genre; $('yearInput').value=game.year; $('protonInput').value=game.proton; $('appIdInput').value=game.appId; $('developerInput').value=game.developer; $('publisherInput').value=game.publisher; $('descriptionInput').value=game.description; $('torrentInput').value=game.torrentUrl||''; $('favoriteInput').checked=game.favorite; $('gameForm').dataset.cover=game.cover; }
  else {$('gameForm').dataset.cover='';$('torrentInput').value='';}
  updatePriorityVisibility();
  $('protonDbHint').textContent=game?.proton&&game.proton!=='Sem dados'?`Classificação salva: ${game.proton}.`:'Sem dados consultados.';
  if(searching){$('steamQuery').value='';$('steamResults').innerHTML='';$('steamHint').textContent='Os dados públicos e a capa serão salvos na sua biblioteca local.';}
  if(!dialog.open) dialog.showModal();
}
$('manualFormBtn').addEventListener('click',()=>openForm(null,true));

function protonTierLabel(tier){const labels={platinum:'Platinum',gold:'Gold',silver:'Silver',bronze:'Bronze',borked:'Borked'};return labels[String(tier||'').toLowerCase()]||'Sem dados';}
async function queryProtonDb(appId=$('appIdInput').value.trim()){
  if(!/^\d+$/.test(appId)){ $('protonDbHint').textContent='Informe um AppID Steam válido para consultar.';return; }
  const button=$('protonDbBtn');button.disabled=true;$('protonDbHint').textContent='Consultando relatórios do ProtonDB…';
  try{
    const summary=await invoke('get_protondb_summary',{appId});
    if(!summary){$('protonInput').value='Sem dados';$('protonDbHint').textContent='Sem relatórios no ProtonDB. Isso não significa que o jogo seja incompatível.';return;}
    const tier=protonTierLabel(summary.tier);$('protonInput').value=tier;
    const confidence=summary.confidence?` · confiança ${summary.confidence}`:'';
    const score=Number.isFinite(summary.score)?` · nota ${summary.score.toFixed(2)}`:'';
    $('protonDbHint').textContent=`${summary.total} ${summary.total===1?'relatório':'relatórios'} · ${tier}${confidence}${score}. Salve o jogo para guardar a classificação.`;
  }catch(e){$('protonDbHint').textContent=`Não foi possível consultar o ProtonDB: ${e}`;}
  finally{button.disabled=false;}
}
$('protonDbBtn').addEventListener('click',()=>queryProtonDb());

function showCoverPreview(data,label){
  const row=$('coverPreviewRow');const image=$('coverPreview');
  if(!data?.startsWith('data:image/')){row.classList.add('hidden');image.innerHTML='';return;}
  image.innerHTML=`<img src="${data}" alt="Prévia da capa">`;
  $('coverPreviewTitle').textContent='Capa pronta';$('coverPreviewStatus').textContent=label;
  row.classList.remove('hidden');
}
$('coverInput').addEventListener('change',()=>{
  const file=$('coverInput').files[0];if(!file)return;
  if(file.size>5*1024*1024){$('formError').textContent='A capa deve ter até 5 MB.';$('coverInput').value='';return;}
  const reader=new FileReader();reader.onload=()=>showCoverPreview(reader.result,'Imagem selecionada para salvar localmente');reader.readAsDataURL(file);
});
function steamAppIdFromInput(input){
  const raw=input.trim();if(/^\d+$/.test(raw))return raw;
  if(!/^(https?:\/\/|www\.store\.steampowered\.com\/|store\.steampowered\.com\/)/i.test(raw))return null;
  try{const url=new URL(/^https?:\/\//i.test(raw)?raw:`https://${raw}`);if(!['store.steampowered.com','www.store.steampowered.com'].includes(url.hostname.toLowerCase()))return null;return url.pathname.match(/^\/app\/(\d+)(?:\/|$)/)?.[1]||null;}catch{return null;}
}
function isSteamLink(input){return /^(https?:\/\/|www\.store\.steampowered\.com\/|store\.steampowered\.com\/)/i.test(input.trim());}
async function importSteamGame(appId){
  $('steamSearchBtn').disabled=true;$('steamHint').textContent='Buscando os detalhes e a capa na Steam…';
  try{
    const existing=await invoke('find_steam_game',{appId});
    if(existing){dialog.close();openForm(existing,true);notify('Esse jogo já está na biblioteca.');return;}
    const game=await invoke('get_steam_game',{appId});if(!game.cover)notify('A Steam não retornou uma capa; você pode escolher uma imagem manualmente.');openForm(game,true);if(game.appId)void queryProtonDb(game.appId);
  }catch(err){$('steamHint').textContent=`Não foi possível importar: ${err}`;}
  finally{$('steamSearchBtn').disabled=false;}
}
async function searchSteam(){
  const query=$('steamQuery').value.trim();if(!query){$('steamHint').textContent='Digite o nome, AppID ou link da Steam.';return;}
  const appId=steamAppIdFromInput(query);
  if(appId){await importSteamGame(appId);return;}
  if(isSteamLink(query)){$('steamHint').textContent='Use um link no formato store.steampowered.com/app/12345/.';return;}
  $('steamSearchBtn').disabled=true;$('steamHint').textContent='Buscando jogos na Steam…';$('steamResults').innerHTML='';
  try{
    const results=await invoke('search_steam_games',{query});
    if(!results.length){$('steamHint').textContent='Nenhum jogo encontrado. Confira o nome ou cadastre manualmente.';return;}
    $('steamHint').textContent=`${results.length} resultados · escolha o jogo para conferir os detalhes.`;
    for(const item of results){
      const row=document.createElement('div');row.className='steam-result';
      const title=document.createElement('div');title.className='steam-result-title';title.textContent=item.title;
      const id=document.createElement('div');id.className='steam-result-id';id.textContent=`AppID ${item.appId}`;
      const info=document.createElement('div');info.className='steam-result-info';info.append(title,id);
      const button=document.createElement('button');button.className='toolbar-button import-result';button.textContent='Importar';button.addEventListener('click',()=>importSteamGame(item.appId));
      row.append(info,button);$('steamResults').append(row);
    }
  }catch(err){$('steamHint').textContent=`A busca da Steam falhou: ${err}`;}
  finally{$('steamSearchBtn').disabled=false;}
}
$('steamSearchBtn').addEventListener('click',searchSteam);
$('steamQuery').addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();searchSteam();}});
$('gameForm').addEventListener('submit', async (e) => {
  e.preventDefault(); $('formError').textContent='';
  let cover = $('gameForm').dataset.cover || '';
  const file = $('coverInput').files[0];
  if(file){
    if(file.size > 5*1024*1024){$('formError').textContent='A capa deve ter até 5 MB.';return;}
    cover = await new Promise((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(r.result);r.onerror=reject;r.readAsDataURL(file);});
  }
  const status=$('gameStatus').value;
  const game={id:Number($('gameId').value),title:$('titleInput').value.trim(),status,priority:status==='Na lista' ? $('gamePriority').value : '',genre:$('genreInput').value.trim(),year:$('yearInput').value.trim(),proton:$('protonInput').value,cover,developer:$('developerInput').value.trim(),publisher:$('publisherInput').value.trim(),appId:$('appIdInput').value.trim(),description:$('descriptionInput').value.trim(),torrentUrl:$('torrentInput').value.trim(),favorite:$('favoriteInput').checked,addedAt:'',updatedAt:''};
  try{await invoke('save_game',{game});dialog.close();await refresh();notify(game.id?'Jogo atualizado.':'Jogo adicionado à biblioteca.');}catch(err){$('formError').textContent=`Não foi possível salvar: ${err}`;}
});
$('deleteBtn').addEventListener('click', async()=>{const id=Number($('gameId').value);try{await invoke('delete_game',{id});dialog.close();await refresh();notify('Jogo removido da biblioteca.');}catch(e){$('formError').textContent=`Não foi possível excluir: ${e}`;}});
function askRemove(game){
  pendingRemoval=game;
  const fromWishlist=game.status==='Na lista';
  $('removeTitle').textContent=fromWishlist?'Remover da lista de desejos?':'Remover jogo da biblioteca?';
  $('removeMessage').textContent=`“${game.title}” será removido da sua biblioteca local. Essa ação não pode ser desfeita.`;
  $('confirmRemoveBtn').textContent=fromWishlist?'Remover da lista':'Remover jogo';
  $('removeConfirmDialog').showModal();
}
$('cancelRemoveBtn').addEventListener('click',()=>$('removeConfirmDialog').close());
$('confirmRemoveBtn').addEventListener('click',async()=>{
  if(!pendingRemoval)return;
  try{await invoke('delete_game',{id:pendingRemoval.id});$('removeConfirmDialog').close();notify(pendingRemoval.status==='Na lista'?'Jogo removido da lista de desejos.':'Jogo removido da biblioteca.');pendingRemoval=null;await refresh();}
  catch(e){notify(`Não foi possível remover o jogo: ${e}`);}
});
function openDetails(game){
  currentDetail=game;$('detailTitle').textContent=game.title;$('detailDescription').textContent=game.description||'Nenhuma descrição cadastrada.';
  $('detailCover').innerHTML=parseCover(game.cover,game.title);$('detailTags').innerHTML=[game.status,game.status==='Na lista'?game.priority:'',game.proton,game.genre].filter(Boolean).map(v=>`<span>${escapeHtml(v)}</span>`).join('');
  $('detailMeta').innerHTML=`<div><span>DESENVOLVEDORA</span>${escapeHtml(game.developer||'—')}</div><div><span>LANÇAMENTO</span>${escapeHtml(game.year||'—')}</div><div><span>APPID STEAM</span>${escapeHtml(game.appId||'—')}</div><div><span>DISTRIBUIDORA</span>${escapeHtml(game.publisher||'—')}</div>`;
  $('openSteamBtn').classList.toggle('hidden',!game.appId);
  $('openProtonBtn').classList.toggle('hidden',!game.appId);
  $('sendTorrentBtn').classList.toggle('hidden',!game.torrentUrl);
  $('detailFavorite').textContent=game.favorite?'★ Remover favorito':'☆ Favoritar';detailDialog.showModal();
}
$('detailEdit').addEventListener('click',()=>{detailDialog.close();openForm(currentDetail);});
$('detailFavorite').addEventListener('click',async()=>{await invoke('set_favorite',{id:currentDetail.id,value:!currentDetail.favorite});detailDialog.close();await refresh();notify('Favoritos atualizados.');});
$('openSteamBtn').addEventListener('click',async()=>{try{await invoke('open_steam_page',{appId:currentDetail.appId});}catch(e){notify(`Não foi possível abrir a Steam: ${e}`);}});
$('openProtonBtn').addEventListener('click',async()=>{try{await invoke('open_protondb_page',{appId:currentDetail.appId});}catch(e){notify(`Não foi possível abrir o ProtonDB: ${e}`);}});
$('sendTorrentBtn').addEventListener('click',()=>{
  if(!currentDetail?.torrentUrl)return;
  pendingTorrentGameId=currentDetail.id;
  $('torrentConfirmText').textContent=`“${currentDetail.title}” será enviado ao qBittorrent e o download começará imediatamente.`;
  $('torrentConfirmDialog').showModal();
});
$('cancelTorrentBtn').addEventListener('click',()=>$('torrentConfirmDialog').close());
$('confirmTorrentBtn').addEventListener('click',async()=>{
  if(!pendingTorrentGameId)return;
  const button=$('confirmTorrentBtn');setButtonLoading(button,true,'Adicionando…');
  try{if(qbitStartupPromise){const ready=await qbitStartupPromise;if(!ready)throw new Error(qbitStartupError||'Não foi possível iniciar o qBittorrent.');}await invoke('add_game_torrent_to_qbittorrent',{id:pendingTorrentGameId});$('torrentConfirmDialog').close();notify('Download iniciado · status atualizado para Baixando.');detailDialog.close();await refresh();openQbitDialog();await refreshQbitDownloads(true);}
  catch(e){$('torrentConfirmDialog').close();notify(`Não foi possível enviar o download: ${e}`);openQbitDialog();}
  finally{setButtonLoading(button,false);pendingTorrentGameId=null;}
});
function formatBytes(value){let amount=Math.max(0,Number(value)||0),unit=0;const units=['B','KB','MB','GB','TB'];while(amount>=1024&&unit<units.length-1){amount/=1024;unit++;}return `${amount.toFixed(unit===0?0:1)} ${units[unit]}`;}
function formatEta(seconds){if(!Number.isFinite(seconds)||seconds<0||seconds>31536000)return '—';const h=Math.floor(seconds/3600),m=Math.floor((seconds%3600)/60),s=Math.floor(seconds%60);return h?`${h}h ${m}min`:`${m}min ${s}s`;}
function qbitDisplayState(torrent){
  const state=torrent.state||'unknown';
  if(state==='error')return 'Erro';
  if(state==='missingFiles')return 'Arquivos ausentes';
  if(Number(torrent.progress)>=0.999){
    if(['pausedUP','stoppedUP'].includes(state))return 'Concluído · semeio pausado';
    if(['uploading','forcedUP'].includes(state))return 'Concluído · semeando';
    if(state==='stalledUP')return 'Concluído · aguardando pares';
    if(state==='queuedUP')return 'Concluído · na fila de semeio';
    if(['checkingUP','checkingResumeData'].includes(state))return 'Concluído · verificando arquivos';
    return 'Concluído';
  }
  const labels={downloading:'Baixando',forcedDL:'Baixando',stalledDL:'Aguardando fontes',metaDL:'Obtendo metadados',pausedDL:'Pausado',stoppedDL:'Pausado',queuedDL:'Na fila',checkingDL:'Verificando',checkingResumeData:'Verificando dados',allocating:'Preparando espaço',moving:'Movendo arquivos',unknown:'Preparando'};
  return labels[state]||'Preparando';
}
function renderQbitDownloads(items){
  updateQbitActivity(items);
  const root=$('qbitDownloads');root.replaceChildren();
  if(!items.length){const empty=document.createElement('div');empty.className='qbit-empty';empty.textContent='Não há downloads no qBittorrent.';root.append(empty);return;}
  for(const torrent of items){
    const completed=Number(torrent.progress)>=0.999;const pausedStates=new Set(['pausedDL','pausedUP','stoppedDL','stoppedUP']);const paused=pausedStates.has(torrent.state);
    const row=document.createElement('article');row.className=`qbit-download${completed?' is-complete':''}`;
    const name=document.createElement('strong');name.className='qbit-download-name';name.textContent=torrent.name||'Torrent sem nome';
    const state=document.createElement('span');state.className=`qbit-download-state${completed?' is-complete':''}`;state.textContent=qbitDisplayState(torrent);
    const track=document.createElement('div');track.className='qbit-progress';const bar=document.createElement('span');const percent=Math.max(0,Math.min(100,(Number(torrent.progress)||0)*100));bar.style.width=`${percent}%`;track.append(bar);
    const meta=document.createElement('small');meta.className='qbit-download-meta';
    meta.textContent=completed?`${percent.toFixed(1)}% · ${formatBytes(torrent.size)} · envio ${formatBytes(torrent.uploadSpeed)}/s`:`${percent.toFixed(1)}% · ${formatBytes(torrent.size)} · download ${formatBytes(torrent.downloadSpeed)}/s${torrent.state==='downloading'&&formatEta(Number(torrent.eta))!=='—'?` · restam ${formatEta(Number(torrent.eta))}`:''}`;
    const actions=document.createElement('div');actions.className='qbit-download-actions';
    const toggle=document.createElement('button');toggle.type='button';toggle.className='toolbar-button';toggle.dataset.qbitAction=paused?'resume':'pause';toggle.dataset.hash=torrent.hash;toggle.textContent=paused?(completed?'Retomar semeio':'Retomar'):(completed?'Parar semeio':'Pausar');toggle.setAttribute('aria-label',`${paused?'Retomar':'Pausar'} ${torrent.name}`);
    const remove=document.createElement('button');remove.type='button';remove.className='toolbar-button qbit-remove-button';remove.dataset.qbitRemove=torrent.hash;remove.textContent='Remover…';remove.setAttribute('aria-label',`Remover ${torrent.name} do qBittorrent`);
    actions.append(toggle,remove);row.append(name,state,track,meta,actions);root.append(row);
  }
  root.querySelectorAll('[data-qbit-action]').forEach(button=>button.addEventListener('click',()=>controlQbitTorrent(button)));
  root.querySelectorAll('[data-qbit-remove]').forEach(button=>button.addEventListener('click',()=>askRemoveQbit(button.dataset.qbitRemove,button.getAttribute('aria-label').replace(/^Remover | do qBittorrent$/g,''))));
}
async function controlQbitTorrent(button){
  const action=button.dataset.qbitAction;setButtonLoading(button,true,action==='pause'?'Pausando…':'Retomando…');
  try{await invoke('control_qbittorrent_torrent',{hash:button.dataset.hash,action});setQbitStatus(action==='pause'?'Download pausado.':'Download retomado.','success');await refreshQbitDownloads();}
  catch(error){setQbitStatus(`Não foi possível controlar o download: ${error}`,'error');}
  finally{setButtonLoading(button,false);}
}
function askRemoveQbit(hash,name){
  pendingQbitRemoval={hash,name};$('qbitRemoveMessage').textContent=`Remover “${name}” da fila do qBittorrent? Você pode manter os arquivos baixados ou excluí-los junto com o torrent.`;$('qbitRemoveDialog').showModal();
}
async function confirmRemoveQbit(deleteFiles,button){
  if(!pendingQbitRemoval)return;const removal=pendingQbitRemoval;
  setButtonLoading(button,true,deleteFiles?'Excluindo…':'Removendo…');$('cancelQbitRemove').disabled=true;$('removeQbitKeepFiles').disabled=true;$('removeQbitDeleteFiles').disabled=true;
  try{await invoke('remove_qbittorrent_torrent',{hash:removal.hash,deleteFiles});$('qbitRemoveDialog').close();pendingQbitRemoval=null;notify(deleteFiles?'Torrent removido e arquivos excluídos.':'Torrent removido; arquivos mantidos.');await refreshQbitDownloads();}
  catch(error){setQbitStatus(`Não foi possível remover o torrent: ${error}`,'error');}
  finally{for(const item of [$('cancelQbitRemove'),$('removeQbitKeepFiles'),$('removeQbitDeleteFiles')])item.disabled=false;setButtonLoading(button,false);}
}
function updateQbitActivity(items){
  const pausedStates=new Set(['pausedDL','pausedUP','error','missingFiles']);
  const seedingStates=new Set(['uploading','stalledUP','queuedUP','forcedUP']);
  const downloading=items.filter(torrent=>Number(torrent.progress)<0.999&&!pausedStates.has(torrent.state)).length;
  const seeding=items.filter(torrent=>Number(torrent.progress)>=0.999&&seedingStates.has(torrent.state)).length;
  const badge=$('qbitActivityBadge');
  badge.classList.toggle('hidden',downloading+seeding===0);
  badge.textContent=`↓${downloading} ↑${seeding}`;
  badge.setAttribute('aria-label',`${downloading} ${downloading===1?'download':'downloads'} em andamento; ${seeding} ${seeding===1?'jogo semeando':'jogos semeando'}`);
  $('qbitNav').title=downloading+seeding?`${downloading} baixando · ${seeding} semeando`:'Sem downloads ou semeamentos ativos';
}
async function refreshQbitActivity(){
  if(qbitActivityBusy||!qbitStartupDone||qbitStartupError)return;
  qbitActivityBusy=true;
  try{const snapshot=await invoke('list_qbittorrent_downloads');updateQbitActivity(snapshot.items);if(snapshot.updatedGameIds?.length)await refresh();}
  catch{updateQbitActivity([]);}
  finally{qbitActivityBusy=false;}
}
async function refreshQbitDownloads(showLoading = false){
  if(qbitRefreshBusy)return;
  qbitRefreshBusy=true;
  const button=$('qbitRefreshBtn');
  if(showLoading){setButtonLoading(button,true,'Atualizando…');setQbitStatus('Buscando a fila de downloads…','loading');renderQbitLoading();}
  try{if(qbitStartupPromise){const ready=await qbitStartupPromise;if(!ready)throw new Error(qbitStartupError||'Não foi possível iniciar o qBittorrent.');}const snapshot=await invoke('list_qbittorrent_downloads');renderQbitDownloads(snapshot.items);setQbitStatus(`${snapshot.items.length} ${snapshot.items.length===1?'download':'downloads'} na fila.`, 'success');if(snapshot.updatedGameIds?.length)await refresh();if(!qbitRefreshTimer&&$('qbitDialog').open)qbitRefreshTimer=setInterval(()=>{if($('qbitDialog').open)refreshQbitDownloads();},4000);}
  catch(e){setQbitStatus(String(e),'error');if(showLoading)renderQbitNotice('Não foi possível carregar os downloads. Você pode tentar novamente.');clearInterval(qbitRefreshTimer);qbitRefreshTimer=null;}
  finally{if(showLoading)setButtonLoading(button,false);qbitRefreshBusy=false;}
}
function renderQbitLoading(){
  const root=$('qbitDownloads');root.replaceChildren();
  for(let index=0;index<3;index++){const skeleton=document.createElement('div');skeleton.className='qbit-skeleton';skeleton.setAttribute('aria-hidden','true');root.append(skeleton);}
}
function renderQbitNotice(message){
  const root=$('qbitDownloads');root.replaceChildren();
  const notice=document.createElement('div');notice.className='qbit-empty';notice.textContent=message;root.append(notice);
}
async function openQbitDialog(){
  if(!$('qbitDialog').open)$('qbitDialog').showModal();
  if(!qbitStartupDone){setQbitStatus('Preparando os downloads em segundo plano…','loading');renderQbitLoading();}
  try{if(qbitStartupPromise){const ready=await qbitStartupPromise;if(!ready)throw new Error(qbitStartupError||'Não foi possível iniciar o qBittorrent.');}await refreshQbitDownloads(true);}
  catch(e){setQbitStatus(String(e),'error');renderQbitNotice('Não foi possível carregar os downloads. Você pode tentar novamente.');}
}
$('qbitNav').addEventListener('click',openQbitDialog);
$('qbitDialog').addEventListener('close',()=>{clearInterval(qbitRefreshTimer);qbitRefreshTimer=null;});
$('qbitConnectBtn').addEventListener('click',async()=>{
  const button=$('qbitConnectBtn');setButtonLoading(button,true,'Preparando…');
  try{const version=(!qbitStartupDone&&qbitStartupPromise)?await qbitStartupPromise:await beginQbitStartup();if(!version)throw new Error(qbitStartupError||'Não foi possível iniciar o qBittorrent.');await refreshQbitDownloads(true);}
  catch(e){setQbitStatus(String(e),'error');renderQbitNotice('Não foi possível iniciar o qBittorrent. Você pode tentar novamente.');}
  finally{setButtonLoading(button,false);}
});
$('qbitRefreshBtn').addEventListener('click',()=>refreshQbitDownloads(true));
$('cancelQbitRemove').addEventListener('click',()=>{$('qbitRemoveDialog').close();pendingQbitRemoval=null;});
$('removeQbitKeepFiles').addEventListener('click',event=>confirmRemoveQbit(false,event.currentTarget));
$('removeQbitDeleteFiles').addEventListener('click',event=>confirmRemoveQbit(true,event.currentTarget));
$('qbitOpenClientBtn').addEventListener('click',async()=>{try{await invoke('open_qbittorrent_client');notify('qBittorrent aberto.');}catch(e){$('qbitStatus').textContent=String(e);}});
$('backupNav').addEventListener('click',()=>$('backupDialog').showModal());
$('settingsNav').addEventListener('click',()=>$('settingsDialog').showModal());
$('closeSettingsBtn').addEventListener('click',()=>$('settingsDialog').close());
$('openUninstallConfirmBtn').addEventListener('click',()=>{
  $('uninstallConfirmText').textContent=$('removeUserDataCheck').checked
    ?'O aplicativo, os atalhos e também sua biblioteca local, capas, cache e preferências serão apagados permanentemente.'
    :'O aplicativo, o lançador, o ícone e o atalho da área de trabalho serão removidos. Sua biblioteca local será preservada.';
  $('settingsDialog').close();
  $('uninstallConfirmDialog').showModal();
});
$('cancelUninstallBtn').addEventListener('click',()=>{
  $('uninstallConfirmDialog').close();
  $('settingsDialog').showModal();
});
$('confirmUninstallBtn').addEventListener('click',async event=>{
  const button=event.currentTarget;
  setButtonLoading(button,true,'Removendo…');
  try{
    await invoke('uninstall_desire_vault',{removeUserData:$('removeUserDataCheck').checked});
    $('uninstallConfirmDialog').close();
    $('settingsDialog').close();
    $('uninstallCompleteText').textContent=$('removeUserDataCheck').checked
      ?'O aplicativo, os atalhos e os dados locais foram removidos deste usuário.'
      :'O aplicativo e os atalhos foram removidos. Sua biblioteca permanece salva neste dispositivo.';
    $('uninstallCompleteDialog').showModal();
  }catch(error){
    $('uninstallConfirmText').textContent=`A desinstalação não foi concluída: ${error}`;
    $('uninstallConfirmText').classList.add('form-error');
  }finally{setButtonLoading(button,false);}
});
$('finishUninstallBtn').addEventListener('click',async()=>{
  try{await invoke('close_window');}catch(error){notify(`A desinstalação terminou, mas não foi possível fechar a janela: ${error}`);}
});
$('exportBackupBtn').addEventListener('click',async()=>{
  const button=$('exportBackupBtn');setButtonLoading(button,true,'Preparando arquivo…');$('backupStatus').textContent='Montando um backup da biblioteca e das capas…';
  try{const suggestedName=`DesireVault-backup-${new Date().toISOString().slice(0,10)}.json`;const saved=await invoke('save_library_backup',{suggestedName});if(!saved){$('backupStatus').textContent='Salvamento cancelado. Nenhum arquivo foi criado.';return;}$('backupStatus').textContent=`Backup salvo com ${saved.games} ${saved.games===1?'jogo':'jogos'} em: ${saved.path}`;notify('Backup salvo no local escolhido.');}
  catch(error){$('backupStatus').textContent=`Não foi possível exportar o backup: ${error}`;}
  finally{setButtonLoading(button,false);}
});
$('chooseBackupBtn').addEventListener('click',()=>$('backupFileInput').click());
$('backupFileInput').addEventListener('change',async event=>{
  const file=event.target.files?.[0];event.target.value='';if(!file)return;
  try{if(file.size>128*1024*1024)throw new Error('O arquivo excede o limite de 128 MB.');const backup=JSON.parse(await file.text());if(backup.format!=='desire-vault-library'||backup.version!==1||!Array.isArray(backup.games))throw new Error('Este arquivo não parece ser um backup compatível do Desire Vault.');pendingRestoreBackup=backup;$('restoreConfirmMessage').textContent=`O arquivo contém ${backup.games.length} ${backup.games.length===1?'jogo':'jogos'}. A biblioteca atual será substituída. Uma cópia da biblioteca atual será guardada localmente antes da restauração.`;$('restoreConfirmDialog').showModal();}
  catch(error){$('backupStatus').textContent=`Não foi possível abrir o backup: ${error.message||error}`;}
});
$('cancelRestoreBtn').addEventListener('click',()=>{$('restoreConfirmDialog').close();pendingRestoreBackup=null;});
$('confirmRestoreBtn').addEventListener('click',async event=>{
  if(!pendingRestoreBackup)return;const button=event.currentTarget;setButtonLoading(button,true,'Restaurando…');
  try{const count=await invoke('restore_library_backup',{backup:pendingRestoreBackup});pendingRestoreBackup=null;$('restoreConfirmDialog').close();$('backupDialog').close();await refresh();notify(`Biblioteca restaurada: ${count} ${count===1?'jogo':'jogos'}.`);}
  catch(error){$('restoreConfirmMessage').textContent=`A restauração não foi concluída: ${error}`;}
  finally{setButtonLoading(button,false);}
});
$('backBtn').addEventListener('click',()=>{section='Todos os jogos';favoritesOnly=false;refresh();});
$('backBtn').addEventListener('click',()=>{section='Todos os jogos';favoritesOnly=false;refresh();});
$('forwardBtn').addEventListener('click',()=>notify('Você está na biblioteca.'));
const windowButtons = document.querySelectorAll('.window-actions .icon-button');
windowButtons[0]?.addEventListener('click',async()=>{try{await invoke('minimize_window');}catch(e){notify(`Não foi possível minimizar a janela: ${e}`);}});
windowButtons[1]?.addEventListener('click',async()=>{try{await invoke('toggle_maximize_window');}catch(e){notify(`Não foi possível maximizar a janela: ${e}`);}});
windowButtons[2]?.addEventListener('click',async()=>{try{await invoke('close_window');}catch(e){notify(`Não foi possível fechar a janela: ${e}`);}});
document.addEventListener('dragstart',event=>{if(event.target instanceof HTMLImageElement)event.preventDefault();},true);
document.querySelectorAll('.resize-hitbox').forEach(handle=>handle.addEventListener('pointerdown',async event=>{
  if(event.button!==0)return;
  try{await window.__TAURI__?.window?.getCurrentWindow?.().startResizeDragging(handle.dataset.resizeDirection);}
  catch(e){console.warn('Redimensionamento da janela indisponível:',e);}
}));
beginQbitStartup();
refresh();
invoke?.('take_startup_notice').then(message=>{if(message)notify(message);}).catch(error=>console.warn('Aviso de restauração automática indisponível:',error));
document.addEventListener('click',()=>document.querySelectorAll('.card-menu:not(.hidden)').forEach(menu=>{menu.classList.add('hidden');menu.previousElementSibling?.setAttribute('aria-expanded','false');}));
