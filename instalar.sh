#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
gui="$root/outputs/GameLibraryManager/desire-vault-installer"

if [[ -x "$gui" && -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
  exec "$gui" "$root"
fi

app="$root/outputs/GameLibraryManager/game-library-manager"
icon="$root/outputs/GameLibraryManager/icon.png"
if [[ ! -x "$app" || ! -f "$icon" ]]; then
  printf 'Pacote incompleto: não encontrei o aplicativo ou o ícone.\n' >&2
  exit 1
fi
home_dir="${HOME:?Não encontrei sua pasta pessoal}"
data_dir="${XDG_DATA_HOME:-$home_dir/.local/share}"
install -Dm755 "$app" "$home_dir/.local/bin/desire-vault"
install -Dm644 "$icon" "$data_dir/icons/hicolor/256x256/apps/desire-vault.png"
mkdir -p "$data_dir/applications"
cat > "$data_dir/applications/desire-vault.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Desire Vault
GenericName=Biblioteca de jogos
Comment=Organize sua biblioteca pessoal de jogos
Exec=$home_dir/.local/bin/desire-vault
TryExec=$home_dir/.local/bin/desire-vault
Icon=$data_dir/icons/hicolor/256x256/apps/desire-vault.png
Terminal=false
StartupNotify=true
DBusActivatable=false
Categories=Game;
Keywords=jogos;biblioteca;game;library;
DESKTOP
chmod 644 "$data_dir/applications/desire-vault.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$data_dir/applications" >/dev/null 2>&1 || true
command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache --force --ignore-theme-index "$data_dir/icons/hicolor" >/dev/null 2>&1 || true
printf 'Desire Vault instalado em %s/.local/bin/desire-vault\n' "$home_dir"
printf 'Atalho do menu atualizado; use o instalador gráfico para escolher um atalho na área de trabalho ou restaurar um backup.\n'
