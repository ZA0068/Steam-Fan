#!/bin/sh
# Runs on the NAS as root (deploy-nas.sh calls it through sudo):
#   nas-up.sh load    load the image the PC sent (../image.tar.gz) and (re)start the container
#   nas-up.sh build   build the image here and (re)start the container
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
        "$D" load -i ../image.tar.gz
        rm -f ../image.tar.gz
        compose up -d --no-build --force-recreate
        ;;
    build)
        compose up -d --build
        ;;
    *)
        echo "usage: nas-up.sh load|build"
        exit 1
        ;;
esac
"$D" image prune -f >/dev/null 2>&1 || true
