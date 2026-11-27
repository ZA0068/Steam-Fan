#!/bin/sh
# Runs on the NAS as root (deploy-nas.sh calls it through sudo):
#   nas-up.sh load    load the image the PC sent (../image.tar.gz) and (re)start the container
#   nas-up.sh build   build the image here and (re)start the container
#   nas-up.sh pull    get the image GitHub built (ghcr.io) and (re)start the container
set -e
cd "$(dirname "$0")"
D=$(command -v docker 2>/dev/null || echo /usr/local/bin/docker)
if "$D" compose version >/dev/null 2>&1; then
    compose() { "$D" compose "$@"; }
else
    DC=$(command -v docker-compose 2>/dev/null || echo /usr/local/bin/docker-compose)
    compose() { "$DC" "$@"; }
fi
case "$1" in
    load)
        # Podman names its images localhost/<name>: tag it with the name compose.yaml uses
        LOADED=$("$D" load -i ../image.tar.gz | sed -n 's/^Loaded image: //p' | tail -1)
        rm -f ../image.tar.gz
        if [ -n "$LOADED" ] && [ "$LOADED" != "steam-fan-server:latest" ]; then
            "$D" tag "$LOADED" steam-fan-server:latest
        fi
        compose up -d --no-build --force-recreate
        ;;
    build)
        compose up -d --build
        ;;
    pull)
        "$D" pull ghcr.io/za0068/steam-fan-server:latest
        "$D" tag ghcr.io/za0068/steam-fan-server:latest steam-fan-server:latest
        compose up -d --no-build --force-recreate
        ;;
    *)
        echo "usage: nas-up.sh load|build|pull"
        exit 1
        ;;
esac
"$D" image prune -f >/dev/null 2>&1 || true
