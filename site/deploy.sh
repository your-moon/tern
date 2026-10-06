#!/bin/sh
# Push site/ to grape-2, served by the carrot-soft-tern nginx container (:8094)
# behind Traefik at https://tern.carrot-soft.tech. Live immediately.
set -e
cd "$(dirname "$0")"
rsync -az --delete --exclude deploy.sh --exclude README.md ./ grape-2:/opt/carrot-soft/tern/
