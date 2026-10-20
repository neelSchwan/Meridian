# Builds all three binaries (meridian, meridian-ingest, meridian-server) into one small image.
FROM rust:1-slim-trixie AS build
# riven's HTTP client uses native TLS, which links OpenSSL.
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/meridian /src/target/release/meridian-ingest \
    /src/target/release/meridian-server /usr/local/bin/
ENV MERIDIAN_DB=/data/meridian.db \
    MERIDIAN_ADDR=0.0.0.0:7979
VOLUME /data
EXPOSE 7979
CMD ["meridian-server"]
