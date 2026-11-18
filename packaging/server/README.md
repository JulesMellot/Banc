# Installing the Banc server

The server is one Docker image, `ghcr.io/julesmellot/banc-server`, for amd64 and arm64. Everything goes through one HTTP port, 8080: the API, the web admin and the relay. HTTPS comes from what is in front of it.

The only required setting is `BANC_DOMAIN`, the domain name of the server. Data lives in the volume mounted on `/var/lib/banc`: keep it, it holds the team's database and the server's identity.

## On Coolify

1. Point the domain's DNS record to the Coolify server.
2. In Coolify: **New Resource**, **Docker Compose Empty**, then paste [`coolify.yaml`](coolify.yaml).
3. In the environment variables, set `BANC_DOMAIN` to your domain, for example `banc.mycompany.com`.
4. In the settings of the `banc-server` service, set the domain to `https://banc.mycompany.com:8080`. The `:8080` tells Coolify which port to use, it does not appear in the address.
5. Deploy.
6. Read the first-admin code in the service's terminal: `cat /var/lib/banc/setup-token`. Open `https://banc.mycompany.com` and enter it.

Coolify's proxy may already use UDP 443, so this setup publishes no UDP port: agents reach the server through its relay, over HTTPS.

## On another platform

Run the image with `BANC_DOMAIN`, a volume on `/var/lib/banc`, a tmpfs on `/run/banc` if the filesystem is read-only, and send HTTPS traffic to port 8080. The proxy must allow WebSocket upgrades.

## On a plain VPS

Docker with Compose is enough. Caddy obtains the certificate.

1. Copy `compose.yaml`, `Caddyfile` and `.env.example` to the VPS.
2. `cp .env.example .env`, then set `BANC_DOMAIN` and `BANC_PUBLIC_IP`.
3. Open TCP 80, TCP 443 and UDP 443 on the firewall, nothing else.
4. `docker compose up -d`
5. `docker compose exec banc-server cat /var/lib/banc/setup-token` gives the first-admin code for `https://<domain>`.

## Updating

Pull the new image and restart the service: **Redeploy** on Coolify, `docker compose pull && docker compose up -d` on a plain VPS.
