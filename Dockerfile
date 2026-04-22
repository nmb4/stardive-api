FROM rust:1.95-bookworm AS rust-builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY installers ./installers
COPY eternal ./eternal
RUN cargo build --release -p stardive-api

FROM golang:1.25-bookworm AS freeze-builder
RUN go install github.com/charmbracelet/freeze@latest

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl python3 python3-pip \
  && rm -rf /var/lib/apt/lists/* \
  && pip3 install --break-system-packages --no-cache-dir ddgs

COPY --from=rust-builder /build/target/release/stardive-api /usr/local/bin/stardive-api
COPY --from=freeze-builder /go/bin/freeze /usr/local/bin/freeze
COPY installers /opt/stardive/installers
COPY eternal /opt/stardive/eternal
COPY hooks /hooks

RUN useradd --system --uid 10001 --create-home --home-dir /home/stardive stardive \
  && mkdir -p /storage \
  && chown -R stardive:stardive /storage /opt/stardive /hooks

USER stardive
ENV STARDIVE_BIND_ADDR=0.0.0.0:80
ENV STARDIVE_DATA_DIR=/storage
ENV STARDIVE_INSTALLERS_DIR=/opt/stardive/installers
ENV STARDIVE_ETERNAL_DIR=/opt/stardive/eternal
ENV PATH=/usr/local/bin:/usr/bin:/bin

EXPOSE 80
HEALTHCHECK --interval=30s --timeout=5s --retries=5 CMD curl --fail --silent http://127.0.0.1/up > /dev/null || exit 1

CMD ["/usr/local/bin/stardive-api"]
