#!/bin/sh
# Writes the server configuration from the environment, then starts the server.
#
# BANC_DOMAIN       optional, the domain name of the server; without it, the
#                   server learns it from the request of the first admin
# BANC_PUBLIC_IP    optional, public IPv4 address for direct iroh connections;
#                   without it, agents reach the server through its relay
# BANC_IROH_PORT    optional, public UDP port that leads to UDP 443 here
# BANC_LOG_LEVEL    optional, error, warn, info, debug or trace
set -eu

# Pasted from a browser: https://banc.example.com/
domain=${BANC_DOMAIN:-}
domain=${domain#https://}
domain=${domain#http://}
domain=${domain%%/*}
public_ip=${BANC_PUBLIC_IP:-}
iroh_port=${BANC_IROH_PORT:-443}
log_level=${BANC_LOG_LEVEL:-info}

refuse() {
  echo "$1" >&2
  exit 1
}

case "$domain" in
  *[!A-Za-z0-9.-]*) refuse "BANC_DOMAIN is not a domain name: $BANC_DOMAIN" ;;
esac
case "$public_ip" in
  *[!0-9.]*) refuse "BANC_PUBLIC_IP is not an IPv4 address: $public_ip" ;;
esac
case "$iroh_port" in
  '' | *[!0-9]*) refuse "BANC_IROH_PORT is not a port: $iroh_port" ;;
esac
case "$log_level" in
  error | warn | info | debug | trace) ;;
  *) refuse "BANC_LOG_LEVEL is not a log level: $log_level" ;;
esac

public_addrs=""
if [ -n "$public_ip" ]; then
  public_addrs="\"$public_ip:$iroh_port\""
fi

public_url=""
if [ -n "$domain" ]; then
  public_url="public_url = \"https://$domain\""
fi

config=/run/banc/banc-server.toml
cat >"$config" <<TOML
data_dir = "/var/lib/banc"
$public_url

[http]
listen = "0.0.0.0:8080"
admin_ui = "/usr/share/banc/admin"

[iroh]
listen = "0.0.0.0:443"
public_addrs = [$public_addrs]

[relay]
enabled = true
listen = "127.0.0.1:3340"

[logs]
dir = "/var/lib/banc/logs"
level = "$log_level"
keep_days = 30
TOML

exec /usr/local/bin/banc-server --config "$config" --stderr
