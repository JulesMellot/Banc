# Installing the Banc server

The server is one Docker image, `ghcr.io/julesmellot/banc-server`, for amd64 and arm64. Nothing is compiled on your server. Everything goes through one HTTP port, 8080: the API, the web admin and the relay.

There is no setting to fill in. The server learns its address when you create the first admin. Its data lives in the `banc-data` volume: keep it, it holds the team's database and the server's identity.

## On Coolify

1. Point your domain's DNS record to the Coolify server.
2. **New Resource**, then **Public Repository**, and paste `https://github.com/JulesMellot/Banc`.
3. Set **Build Pack** to **Docker Compose** and continue.
4. Give the `banc` service your domain, in `https://`, for example `https://banc.mycompany.com`.
5. **Deploy**.
6. In the service's **Terminal**, run `cat /var/lib/banc/setup-token`.
7. Open `https://banc.mycompany.com`, enter that code and create your admin account.

## On Dokploy

1. Point your domain's DNS record to the Dokploy server.
2. Create a **Compose** service from the Git repository `https://github.com/JulesMellot/Banc`, branch `main`, path `./docker-compose.yml`.
3. In **Domains**, add your domain for the service `banc`, port `8080`, with HTTPS.
4. **Deploy**, then follow steps 6 and 7 above.

## On another platform

Deploy `docker-compose.yml` from this repository, or run the image with a volume on `/var/lib/banc`, and send HTTPS traffic to port 8080. The proxy must allow WebSocket upgrades and pass the `X-Forwarded-Host` and `X-Forwarded-Proto` headers.

Create the first admin through the `https://` address: the server refuses to learn a plain `http://` one. If the address changes later, set `BANC_DOMAIN` to the new domain and redeploy.

## On a plain VPS

Docker with Compose is enough. Caddy obtains the certificate.

1. Copy `compose.yaml`, `Caddyfile` and `.env.example` to the VPS.
2. `cp .env.example .env`, then set `BANC_DOMAIN` and `BANC_PUBLIC_IP`.
3. Open TCP 80, TCP 443 and UDP 443 on the firewall, nothing else.
4. `docker compose up -d`
5. `docker compose exec banc-server cat /var/lib/banc/setup-token` gives the first-admin code for `https://<domain>`.

## Updating

Redeploy the service on your platform, or run `docker compose pull && docker compose up -d` on a plain VPS.
