#!/usr/bin/env bash
# Build Steam-Fan and install it for this user.
#   ./install.sh            build, install, point the Steam menu entry at Steam-Fan
#   ./install.sh --no-steam-menu   leave Steam's menu entry alone
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

command -v cargo > /dev/null || { echo "Rust is missing: sudo pacman -S rustup && rustup default stable"; exit 1; }
# the window needs WebKitGTK; on Arch: sudo pacman -S --needed webkit2gtk-4.1
if ! pkg-config --exists webkit2gtk-4.1 2> /dev/null; then
    echo "The library window needs WebKitGTK. Install it with:"
    echo "    sudo pacman -S --needed webkit2gtk-4.1"
    exit 1
fi

# The interface (ui/dist) comes ready-built. Rebuild it only if you changed ui/src
# and have Node.js: (cd ui && npm install && npm run build)

echo "== Building (first time takes a few minutes)"
cargo build --release

BIN=$HOME/.local/bin
APPS=$HOME/.local/share/applications
ICONS=$HOME/.local/share/icons/hicolor/scalable/apps
mkdir -p "$BIN" "$APPS" "$ICONS"
install -m 755 target/release/steam-fan "$BIN/steam-fan"
install -m 755 target/release/steam-fan-app "$BIN/steam-fan-app"
install -m 644 packaging/steam-fan.svg "$ICONS/steam-fan.svg"
install -m 644 packaging/steam-fan.desktop "$APPS/steam-fan.desktop"
echo "== Installed steam-fan, steam-fan-app and the Steam-Fan menu entry"

if [[ ${1:-} != --no-steam-menu ]]; then
    # Starting Steam from the menu first adds new shelf games to the library.
    for f in /usr/share/applications/steam.desktop "$HOME/.config/autostart/steam.desktop"; do
        [[ -f $f ]] || continue
        out=$APPS/steam.desktop
        [[ $f == *autostart* ]] && out=$f
        sed -E "s#^Exec=(/usr/bin/)?steam( |$)#Exec=$BIN/steam-fan steam\2#; s#^Exec=$HOME/.local/bin/steam-fangames( |$)#Exec=$BIN/steam-fan steam\1#" \
            "$f" > "$out.new" && mv "$out.new" "$out"
    done
    echo "== Steam's menu entry now starts Steam through Steam-Fan"
fi

# Move Steam's library entries over from the old scripts, then retire the scripts.
old=(fan-sync fan-fetch fan-remove fan-remove-here steam-fangames)
if pgrep -x steam > /dev/null || pgrep -x steamwebhelper > /dev/null; then
    echo "!! Steam is running: the old scripts are kept for now."
    echo "   Quit Steam, then run: steam-fan sync   and run ./install.sh again."
elif "$BIN/steam-fan" sync; then
    for s in "${old[@]}"; do rm -f "$BIN/$s"; done
    rm -f "$HOME/.local/share/kio/servicemenus/fan-games-menu.desktop"
    echo "== Old scripts removed (copies are in legacy/)"
else
    echo "!! steam-fan sync failed; the old scripts are kept. Open Steam-Fan to check the settings."
fi
echo "Done. Open Steam-Fan from the app menu, or run: steam-fan"
