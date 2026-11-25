#!/usr/bin/env bash
# archive.sh - keeps an offline copy of your Steam games on the NAS.
#
# Runs inside the steamcmd/steamcmd container (see README.md for the exact command).
#
#   /scripts/apps.txt    one game per line:  <appid> <platform> <name...>
#                        platform = windows (for Proton) or linux; "#" at the start skips a line
#   /scripts/steam_user  your Steam account name (the login is cached after the first manual login)
#   /archive/<appid>/    the game files + steamapps/appmanifest_<appid>.acf
#
# New purchases (optional, automatic):
#   /scripts/steam_api_key   your Steam Web API key   (https://steamcommunity.com/dev/apikey)
#   /scripts/steam_id        your 17-digit SteamID64
#   With both files present, every run first adds games you own but that aren't in apps.txt yet.
#   Platform: linux if the game has a native Linux build, otherwise windows. Edit the line if needed.
#   To stop archiving a game, put # in front of its line. Don't delete the line, or it's added back.
#
# Optional environment variables:
#   VALIDATE=1        re-check every file (slow; use for a weekly run)
#   MIN_FREE_GB=200   stop when less free space than this is left on the archive volume
#   STOP_AT=2230      do not start a new game after this time (NAS powers off at 23:00)

set -uo pipefail

ARCHIVE=${ARCHIVE:-/archive}
SCRIPTS=${SCRIPTS:-/scripts}
LIST=$SCRIPTS/apps.txt
LOG=$SCRIPTS/archive.log
VALIDATE=${VALIDATE:-0}
MIN_FREE_GB=${MIN_FREE_GB:-200}
STOP_AT=${STOP_AT:-2230}

STEAMCMD=${STEAMCMD:-$(command -v steamcmd || echo /home/steam/steamcmd/steamcmd.sh)}

log() { echo "$(date '+%F %T') $*" | tee -a "$LOG"; }
free_gb() { df -P -BG "$ARCHIVE" | awk 'NR==2 {gsub("G", "", $4); print $4}'; }

[[ -x $STEAMCMD ]] || { log "steamcmd not found"; exit 1; }
[[ -r $SCRIPTS/steam_user ]] || { log "missing $SCRIPTS/steam_user"; exit 1; }
[[ -r $LIST ]] || { log "missing $LIST"; exit 1; }
STEAM_USER=$(tr -d '[:space:]' < $SCRIPTS/steam_user)
[[ -n $STEAM_USER ]] || { log "$SCRIPTS/steam_user is empty"; exit 1; }

ok=0
failed=()
out=$(mktemp)
trap 'rm -f "$out"' EXIT

add_new_games() {
    local keyfile=$SCRIPTS/steam_api_key idfile=$SCRIPTS/steam_id key id owned known
    local added=0 appid name resp linux platform
    key=${STEAM_API_KEY:-}
    id=${STEAM_ID:-}
    [[ -z $key && -r $keyfile ]] && key=$(tr -d '[:space:]' < "$keyfile")
    [[ -z $id && -r $idfile ]] && id=$(tr -d '[:space:]' < "$idfile")
    [[ -n $key && -n $id ]] || return 0

    if ! command -v curl > /dev/null || ! command -v jq > /dev/null; then
        log "installing curl and jq for the new-games check"
        { apt-get update -qq && apt-get install -y -qq curl jq ca-certificates; } > /dev/null 2>&1 \
            || { log "could not install curl/jq: new-games check skipped"; return 0; }
    fi

    owned=$(curl -fsS --max-time 60 \
        "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key=$key&steamid=$id&include_appinfo=1&format=json") \
        || { log "owned-games lookup failed (check steam_api_key and steam_id): new-games check skipped"; return 0; }

    # every appid already in the list, including skipped (#) ones
    known=" $(sed -E 's/^[[:space:]#]*//' "$LIST" | awk '$1 ~ /^[0-9]+$/ {print $1}' | tr '\n' ' ') "
    [[ -s $LIST && $(tail -c1 "$LIST" | wc -l) -eq 0 ]] && echo >> "$LIST"

    while IFS=$'\t' read -r appid name; do
        [[ $known == *" $appid "* ]] && continue
        resp=$(curl -fsS --max-time 30 "https://store.steampowered.com/api/appdetails?appids=$appid&filters=platforms" 2> /dev/null) || resp=""
        linux=$(jq -r --arg id "$appid" \
            'if .[$id].success then (.[$id].data.platforms.linux // false | tostring) else "unknown" end' \
            <<< "$resp" 2> /dev/null) || linux=unknown
        [[ $linux == true ]] && platform=linux || platform=windows
        printf '%s %s %s\n' "$appid" "$platform" "$name" >> "$LIST"
        log "new game added to apps.txt: $appid ($platform) $name"
        known+="$appid "
        added=$((added + 1))
        sleep 1.6   # the store allows ~200 lookups per 5 minutes
    done < <(jq -r '.response.games[]? | "\(.appid)\t\(.name)"' <<< "$owned")
    log "new-games check: $added added"
}

log "=== archive run started (validate=$VALIDATE) ==="
add_new_games

while read -r appid platform name; do
    [[ -z ${appid:-} || $appid == \#* ]] && continue
    if [[ ! $appid =~ ^[0-9]+$ ]]; then
        log "skipping bad line: $appid $platform $name"
        continue
    fi
    platform=${platform:-windows}

    if (( 10#$(date +%H%M) >= 10#$STOP_AT )); then
        log "STOP: it is past $STOP_AT, the rest continues next run"
        break
    fi
    if (( $(free_gb) < MIN_FREE_GB )); then
        log "STOP: less than ${MIN_FREE_GB} GB free on the archive volume"
        break
    fi

    extra=()
    [[ $VALIDATE == 1 ]] && extra=(validate)

    log "updating $appid ($platform) ${name:-}"
    # </dev/null: steamcmd must not read the game list from stdin
    "$STEAMCMD" +@sSteamCmdForcePlatformType "$platform" \
        +force_install_dir "$ARCHIVE/$appid" \
        +login "$STEAM_USER" \
        +app_update "$appid" "${extra[@]}" \
        +quit < /dev/null 2>&1 | tee "$out" >> "$LOG"   # live progress in the log

    if grep -q "Success! App '$appid'" "$out" \
        && [[ -f $ARCHIVE/$appid/steamapps/appmanifest_$appid.acf ]]; then
        ok=$((ok + 1))
        log "ok $appid ${name:-}"
    else
        failed+=("$appid")
        log "FAILED $appid ${name:-} (details above in this log)"
    fi
done < "$LIST"

log "=== done: $ok ok, ${#failed[@]} failed ${failed[*]:-} ==="
