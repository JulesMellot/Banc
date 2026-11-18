# The Banc server and its web admin. Built from the root of the repository:
#   docker build -f packaging/server/Dockerfile -t banc-server:latest .

FROM node:22-trixie-slim AS admin
WORKDIR /src/ui
COPY ui/ ./
RUN corepack enable && pnpm install --frozen-lockfile && pnpm build

# Same version as rust-toolchain.toml: the build stops if they differ.
FROM rust:1.99.1-slim-trixie AS server
WORKDIR /src
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN test "$(rustc -V | cut -d' ' -f2)" = \
    "$(sed -n 's/^channel = "\(.*\)"$/\1/p' /tmp/rust-toolchain.toml)"
COPY Cargo.toml Cargo.lock ./
COPY .cargo/ .cargo/
COPY crates/ crates/
COPY migrations/ migrations/
RUN cargo build --release --locked -p banc-server --bin banc-server

FROM debian:trixie-slim
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin banc \
    && install -d -o banc -g banc -m 0700 /var/lib/banc /run/banc
COPY --from=server /src/target/release/banc-server /usr/local/bin/banc-server
COPY --from=admin /src/ui/dist/ /usr/share/banc/admin/
COPY packaging/server/entrypoint.sh /usr/local/bin/banc-entrypoint
RUN chmod 0755 /usr/local/bin/banc-entrypoint
USER banc
VOLUME /var/lib/banc
# Bound inside the container only: Caddy reaches them over the Compose network.
EXPOSE 8080/tcp 3340/tcp 443/udp
ENTRYPOINT ["/usr/local/bin/banc-entrypoint"]
