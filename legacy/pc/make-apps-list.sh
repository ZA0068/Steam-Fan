#!/usr/bin/env bash
# make-apps-list.sh - writes apps.txt with every game your Steam account owns,
# choosing for each game the same platform your PC's Steam would install:
#
#   linux    the game has a native Linux build and you have not forced Proton on it
#   windows  Windows-only game (runs through Proton), or you forced a Proton version
#            for it under the game's Properties -> Compatibility
#
# Needs curl and jq. Get a Web API key at https://steamcommunity.com/dev/apikey
# (keep it private). Your SteamID64 is the 17-digit number on your Steam account page.
#
# Usage:
#   STEAM_API_KEY=xxxx STEAM_ID=7656119xxxxxxxxxx ./make-apps-list.sh > apps.txt
#
# Optional:
#   STEAM_CONFIG=~/.local/share/Steam/config/config.vdf   where your forced-Proton choices are read from
#   DELAY=1.6    seconds between store lookups (the store allows ~200 lookups per 5 minutes)

set -euo pipefail
: "${STEAM_API_KEY:?set STEAM_API_KEY}"
: "${STEAM_ID:?set STEAM_ID (your 17-digit SteamID64)}"
STEAM_CONFIG=${STEAM_CONFIG:-$HOME/.local/share/Steam/config/config.vdf}
DELAY=${DELAY:-1.6}

# Appids you forced to a Proton version (CompatToolMapping in config.vdf; "0" is the global default)
forced_proton() {
    [[ -r $STEAM_CONFIG ]] || return 0
    awk '
        /"CompatToolMapping"/      { inmap = 1; depth = 0; next }
        inmap && /\{/              { depth++; next }
        inmap && /\}/              { depth--; if (depth == 0) inmap = 0; next }
        inmap && depth == 1 && /^[[:space:]]*"[0-9]+"[[:space:]]*$/ { gsub(/[^0-9]/, ""); app = $0; next }
        inmap && depth == 2 && /"name"/ {
            line = $0; sub(/^[[:space:]]*"name"[[:space:]]*"/, "", line); sub(/".*$/, "", line)
            if (app != "0" && line != "") print app
        }
    ' "$STEAM_CONFIG"
}

has_linux_build() {   # prints true / false / unknown
    local resp
    resp=$(curl -fsS "https://store.steampowered.com/api/appdetails?appids=$1&filters=platforms" 2> /dev/null) \
        || { echo unknown; return; }
    jq -r --arg id "$1" 'if .[$id].success then (.[$id].data.platforms.linux // false | tostring) else "unknown" end' \
        <<< "$resp" 2> /dev/null || echo unknown
}

owned=$(curl -fsS "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key=${STEAM_API_KEY}&steamid=${STEAM_ID}&include_appinfo=1&format=json")
count=$(jq '.response.game_count // 0' <<< "$owned")
(( count > 0 )) || { echo "No games returned. Check the API key and SteamID." >&2; exit 1; }

forced=" $(forced_proton | tr '\n' ' ') "
echo "Checking $count games against the Steam store (about $(( count * ${DELAY%.*} / 60 + 1 )) min)..." >&2

echo "# <appid> <platform> <name>   put # in front of a line to skip that game"
n=0
while IFS=$'\t' read -r appid name; do
    n=$((n + 1))
    printf '\r  %d/%d' "$n" "$count" >&2
    if [[ $forced == *" $appid "* ]]; then
        platform=windows; note="  # you forced Proton for this game"
    else
        case $(has_linux_build "$appid") in
            true)  platform=linux;   note="" ;;
            false) platform=windows; note="" ;;
            *)     platform=windows; note="  # store lookup failed; check platform" ;;
        esac
        sleep "$DELAY"
    fi
    echo "$appid $platform $name$note"
done < <(jq -r '.response.games | sort_by(.name | ascii_downcase)[] | "\(.appid)\t\(.name)"' <<< "$owned")

printf '\r%d games written\n' "$count" >&2
