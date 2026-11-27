#!/usr/bin/env bash
# Put Steam-Fan-Server on the Synology NAS and start it. Run it on the PC:
#
#   docker/deploy-nas.sh                 asks for the NAS login (e.g. zain@10.0.0.36)
#   docker/deploy-nas.sh zain@10.0.0.36
#   docker/deploy-nas.sh zain@10.0.0.36:12345      SSH on another port
#   docker/deploy-nas.sh zain@10.0.0.36 --build-on-nas    compile on the NAS even if this PC has Docker
#
# What it does:
#   1. looks at the NAS: your user and group numbers, the share that holds fangames/, Docker
#   2. copies the project to /volume1/docker/steam-fan-src (without build leftovers)
#   3. writes docker/.env there (user, shelf, data folder) for compose.yaml
#   4. builds the image: on this PC if Docker runs here (minutes), else on the NAS (15-30 min)
#   5. starts the container and prints a pairing code
#
# Needs: SSH turned on in DSM (Control Panel > Terminal & SNMP), your DSM user in the
# administrators group (for sudo), and Container Manager installed. Run it again to update.

set -euo pipefail

NAS="${1:-${SF_NAS:-}}"
BUILD_ON_NAS=0
[[ "${2:-}" == "--build-on-nas" || "${1:-}" == "--build-on-nas" ]] && BUILD_ON_NAS=1
[[ "$NAS" == "--build-on-nas" ]] && NAS=""
if [[ -z "$NAS" ]]; then
    read -r -p "NAS login (user@address, e.g. zain@10.0.0.36): " NAS
fi
[[ "$NAS" == *@* ]] || { echo "Give it as user@address, e.g. zain@10.0.0.36 (or zain@10.0.0.36:12345)"; exit 1; }
# user@address:port: SSH on another port than 22
PORT=22
if [[ "${NAS##*@}" == *:* ]]; then
    PORT="${NAS##*:}"
    NAS="${NAS%:*}"
fi
HOST="${NAS#*@}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC=/volume1/docker/steam-fan-src
DATA=/volume1/docker/steam-fan-server

step() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
# one SSH connection for everything: the password is asked once
SOCK="${XDG_RUNTIME_DIR:-/tmp}/steam-fan-ssh-%r@%h:%p"
SSH=(ssh -p "$PORT" -o ControlMaster=auto -o "ControlPath=$SOCK" -o ControlPersist=120)
cleanup() { "${SSH[@]}" -O exit "$NAS" 2>/dev/null || true; }
trap cleanup EXIT

step "Looking at the NAS ($NAS)"
INFO="$("${SSH[@]}" "$NAS" 'echo "uid=$(id -u) gid=$(id -g)"
    for d in /volume*/*/fangames; do [ -d "$d" ] && echo "shelf=$(dirname "$d")" && echo "owner=$(stat -c "%u %g" "$d")"; done
    D=$(command -v docker 2>/dev/null || ls /usr/local/bin/docker 2>/dev/null || true); echo "docker=$D"
    [ -d /volume1/docker ] && echo "dockershare=yes" || true')"
UID_NAS="$(sed -n 's/^uid=\([0-9]*\) .*/\1/p' <<<"$INFO")"
GID_NAS="$(sed -n 's/.* gid=\([0-9]*\)$/\1/p' <<<"$INFO")"
DOCKER="$(sed -n 's/^docker=//p' <<<"$INFO")"
mapfile -t SHELVES < <(sed -n 's/^shelf=//p' <<<"$INFO")
# the container runs as whoever owns the games, so what it adds stays yours (never root)
OWNER="$(sed -n 's/^owner=//p' <<<"$INFO" | head -1)"
if [[ -n "$OWNER" && "${OWNER%% *}" != 0 ]]; then
    UID_NAS="${OWNER%% *}"
    GID_NAS="${OWNER##* }"
fi
if [[ "$UID_NAS" == 0 ]]; then
    read -r -p "The games belong to root. Which user number should the server run as (your DSM user; 'id' shows it)? " UID_NAS
    read -r -p "And group number (often 100, 'users')? " GID_NAS
fi

[[ -n "$DOCKER" ]] || { echo "Docker isn't on the NAS: install Container Manager from the Package Center first."; exit 1; }
grep -q dockershare=yes <<<"$INFO" || { echo "There's no 'docker' shared folder: Container Manager makes one when it's installed (/volume1/docker)."; exit 1; }

if [[ ${#SHELVES[@]} -eq 1 ]]; then
    SHELF="${SHELVES[0]}"
elif [[ ${#SHELVES[@]} -gt 1 ]]; then
    echo "More than one share has a fangames folder:"
    select SHELF in "${SHELVES[@]}"; do [[ -n "$SHELF" ]] && break; done
else
    read -r -p "Which folder on the NAS is the shelf (holds fangames/, roms/...)? " SHELF
fi
echo "    you: uid $UID_NAS, gid $GID_NAS"
echo "    shelf: $SHELF"
echo "    docker: $DOCKER"

step "Copying the project to $SRC"
tar -C "$ROOT" -czf - \
    --exclude=./target --exclude=./ui/node_modules --exclude=./crates/desktop/gen \
    --exclude=./.git --exclude=./legacy --exclude='*.db' --exclude=./docker/.env . |
    "${SSH[@]}" "$NAS" "mkdir -p '$SRC' '$DATA' && tar -xzf - -C '$SRC'"
# the server's data folder must belong to the user it runs as
"${SSH[@]}" -t "$NAS" "[ \"\$(stat -c %u '$DATA')\" = '$UID_NAS' ] || sudo chown -R '$UID_NAS:$GID_NAS' '$DATA'"
"${SSH[@]}" "$NAS" "cat > '$SRC/docker/.env'" <<EOF
# written by deploy-nas.sh
SF_UID=$UID_NAS
SF_GID=$GID_NAS
SF_SHELF_HOST=$SHELF
SF_DATA_HOST=$DATA
EOF

if [[ $BUILD_ON_NAS -eq 0 ]] && command -v docker >/dev/null && docker info >/dev/null 2>&1; then
    step "Building the image on this PC"
    # Podman stands in for docker on some PCs: build in Docker's format (keeps the health check)
    FORMAT=()
    docker --version 2>/dev/null | grep -qi podman && FORMAT=(--format docker)
    docker build "${FORMAT[@]}" -f "$ROOT/docker/Dockerfile" -t steam-fan-server:latest "$ROOT"
    step "Sending the image to the NAS"
    docker save steam-fan-server:latest | gzip -1 | "${SSH[@]}" "$NAS" "cat > '$SRC/image.tar.gz'"
    step "Starting it (sudo asks for your NAS password)"
    "${SSH[@]}" -t "$NAS" "sudo sh '$SRC/docker/nas-up.sh' load"
else
    step "Building on the NAS and starting it (sudo asks for your NAS password; the first build takes 15-30 minutes)"
    "${SSH[@]}" -t "$NAS" "sudo sh '$SRC/docker/nas-up.sh' build"
fi

step "Checking it"
for _ in $(seq 1 20); do
    if curl -fsS --max-time 3 "http://$HOST:8484/api/health" 2>/dev/null; then echo; break; fi
    sleep 2
done
"${SSH[@]}" -t "$NAS" "sudo '$DOCKER' logs --tail 15 steam-fan-server; echo; sudo '$DOCKER' exec steam-fan-server steam-fan-server pair"

cat <<EOF

Steam-Fan-Server is running on the NAS.
  From anywhere (Tailscale): http://zainnas-1.snowy-rigel.ts.net:8484/api/health
  At home:                   http://$HOST:8484/api/health
Pair a PC with the code above (the app asks for it in step 2), or by hand:
  curl -X POST http://$HOST:8484/api/pair -H 'content-type: application/json' -d '{"code":"<code>","device":"$(hostname)"}'
Update later: run this script again.
EOF
