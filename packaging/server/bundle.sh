#!/bin/sh
# Builds the server image and gathers what the VPS needs in dist/server/.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out="$root/dist/server"

docker build -f "$root/packaging/server/Dockerfile" -t banc-server:latest "$root"
rm -rf "$out"
mkdir -p "$out"
docker save banc-server:latest | gzip >"$out/banc-server-image.tar.gz"
cp "$root/packaging/server/compose.yaml" "$root/packaging/server/Caddyfile" \
  "$root/packaging/server/.env.example" "$out/"
echo "Bundle in $out"
