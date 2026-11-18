#!/bin/sh
# Writes the server configuration from the environment, then starts the server.
set -eu

: "${BANC_DOMAIN:?BANC_DOMAIN is required: the server's domain name}"
: "${BANC_PUBLIC_IP:?BANC_PUBLIC_IP is required: the public IPv4 address of the VPS}"

case "$BANC_DOMAIN" in
  *[!A-Za-z0-9.-]*)
    echo "BANC_DOMAIN is not a domain name: $BANC_DOMAIN" >&2
    exit 1
    ;;
esac
case "$BANC_PUBLIC_IP" in
  *[!0-9.]*)
    echo "BANC_PUBLIC_IP is not an IPv4 address: $BANC_PUBLIC_IP" >&2
    exit 1
    ;;
esac

config=/run/banc/banc-server.toml
cat >"$config" <<TOML
data_dir = "/var/lib/banc"
public_url = "https://$BANC_DOMAIN"

[http]
listen = "0.0.0.0:8080"
admin_ui = "/usr/share/banc/admin"

[iroh]
listen = "0.0.0.0:443"
public_addrs = ["$BANC_PUBLIC_IP:443"]

[relay]
enabled = true
listen = "0.0.0.0:3340"

[logs]
dir = "/var/lib/banc/logs"
level = "${BANC_LOG_LEVEL:-info}"
keep_days = 30
TOML

exec /usr/local/bin/banc-server --config "$config" --stderr
