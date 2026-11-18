#!/bin/sh
# Starts the Compose setup on this machine and checks what a VPS would serve.
# Needs Docker, and ports 80 and 443 free. Usage: smoke-test.sh [image]
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export BANC_IMAGE="${1:-banc-server:latest}"
export BANC_DOMAIN=localhost
export BANC_PUBLIC_IP=127.0.0.1
compose="docker compose -p banc-smoke -f $here/compose.yaml"

alone=banc-smoke-alone

cleanup() {
  $compose logs --no-color --tail 40 >&2 || true
  $compose down -v --timeout 10 >/dev/null 2>&1 || true
  docker rm -f "$alone" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
  echo "FAILED: $1" >&2
  exit 1
}

info() {
  curl -fsSk --max-time 5 https://localhost/api/v1/info
}

wait_for_info() {
  tries=0
  until info >/dev/null 2>&1; do
    tries=$((tries + 1))
    [ "$tries" -lt 60 ] || fail "the API does not answer through Caddy"
    sleep 1
  done
}

$compose up -d --wait --wait-timeout 90 banc-server
$compose up -d caddy
wait_for_info

first=$(info)
echo "$first" | grep -q '"setup_required":true' || fail "first start: $first"
echo "$first" | grep -q '"relay_url":"https://localhost"' || fail "relay URL: $first"
echo "$first" | grep -q '127.0.0.1:443' || fail "public address: $first"

curl -fsSk --max-time 5 https://localhost/ | grep -qi '<!doctype html' ||
  fail "the web admin is not served"
status=$(curl -sk --max-time 5 -o /dev/null -w '%{http_code}' https://localhost/generate_204)
[ "$status" = 204 ] || fail "the relay answers $status on /generate_204"
alt=$(curl -skI --max-time 5 https://localhost/ | grep -i '^alt-svc' || true)
[ -z "$alt" ] || fail "Caddy announces HTTP/3, which belongs to iroh: $alt"

[ "$($compose exec -T banc-server id -u)" = 10001 ] || fail "the server runs as root"
$compose exec -T banc-server test -s /var/lib/banc/setup-token ||
  fail "no setup token written"

# The server keeps its identity when the container is replaced.
$compose up -d --force-recreate --wait --wait-timeout 90 banc-server
wait_for_info
key() { sed -n 's/.*"endpoint_id":"\([^"]*\)".*/\1/p'; }
[ "$(echo "$first" | key)" = "$(info | key)" ] || fail "the server key changed on restart"

# Behind a hosting platform: the image alone, one HTTP port, only the domain.
docker compose -f "$here/coolify.yaml" config -q
docker run -d --name "$alone" --read-only --tmpfs /run/banc:uid=10001,gid=10001,mode=0700 \
  -v /var/lib/banc -e BANC_DOMAIN=https://banc.example.com/ \
  -p 127.0.0.1:18080:8080 "$BANC_IMAGE" >/dev/null
tries=0
until [ "$(docker inspect -f '{{.State.Health.Status}}' "$alone")" = healthy ]; do
  tries=$((tries + 1))
  [ "$tries" -lt 60 ] || fail "the image alone never becomes healthy"
  sleep 1
done
alone_info=$(curl -fsS --max-time 5 http://127.0.0.1:18080/api/v1/info)
echo "$alone_info" | grep -q '"relay_url":"https://banc.example.com"' ||
  fail "relay URL of the image alone: $alone_info"
echo "$alone_info" | grep -q '"direct_addrs":\[\]' ||
  fail "direct addresses of the image alone: $alone_info"
status=$(curl -s --max-time 5 -o /dev/null -w '%{http_code}' http://127.0.0.1:18080/generate_204)
[ "$status" = 204 ] || fail "the HTTP port answers $status on /generate_204"

echo "server image: all checks passed"
