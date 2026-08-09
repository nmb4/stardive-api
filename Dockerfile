FROM rust:1.95-bookworm AS rust-builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY files-webapp.html ./files-webapp.html
COPY notify ./notify
COPY installers ./installers
COPY eternal ./eternal
RUN cargo build --release -p stardive-api

FROM golang:1.25-bookworm AS freeze-builder
RUN go install github.com/charmbracelet/freeze@latest

FROM debian:bookworm-slim AS obscura-downloader
ARG TARGETARCH
ARG OBSCURA_VERSION=v0.2.0
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && case "$TARGETARCH" in \
       amd64) obscura_arch=x86_64-linux ;; \
       arm64) obscura_arch=aarch64-linux ;; \
       *) echo "unsupported Obscura architecture: $TARGETARCH" >&2; exit 1 ;; \
     esac \
  && mkdir -p /out \
  && curl -fsSL "https://github.com/h4ckf0r0day/obscura/releases/download/${OBSCURA_VERSION}/obscura-${obscura_arch}.tar.gz" \
     | tar -xz -C /out \
  && test -x /out/obscura

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl python3 python3-pip \
  && rm -rf /var/lib/apt/lists/* \
  && pip3 install --break-system-packages --no-cache-dir ddgs

COPY --from=rust-builder /build/target/release/stardive-api /usr/local/bin/stardive-api
COPY --from=freeze-builder /go/bin/freeze /usr/local/bin/freeze
COPY --from=obscura-downloader /out/obscura /usr/local/bin/obscura
COPY installers /opt/stardive/installers
COPY eternal /opt/stardive/eternal
COPY hooks /hooks
COPY scripts/run-stardive-with-obscura.sh /usr/local/bin/run-stardive-with-obscura

RUN useradd --system --uid 10001 --create-home --home-dir /home/stardive stardive \
  && mkdir -p /storage \
  && chown -R stardive:stardive /storage /opt/stardive /hooks \
  && chmod 755 /usr/local/bin/run-stardive-with-obscura

USER stardive
ENV STARDIVE_BIND_ADDR=0.0.0.0:80
ENV STARDIVE_DATA_DIR=/storage
ENV STARDIVE_INSTALLERS_DIR=/opt/stardive/installers
ENV STARDIVE_ETERNAL_DIR=/opt/stardive/eternal
ENV STARDIVE_OBSCURA_MCP_URL=http://127.0.0.1:8081/mcp
ENV PATH=/usr/local/bin:/usr/bin:/bin

EXPOSE 80
HEALTHCHECK --interval=30s --timeout=5s --retries=5 CMD curl --fail --silent http://127.0.0.1/up > /dev/null || exit 1

CMD ["/usr/local/bin/run-stardive-with-obscura"]
