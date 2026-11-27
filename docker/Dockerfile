# Steam-Fan-Server image.
# Build from the project folder:  docker build -f docker/Dockerfile -t steam-fan-server .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# the desktop app needs Tauri's system libraries; the server doesn't, so build only it
# two jobs at a time: a NAS has little memory
ENV CARGO_BUILD_JOBS=2
RUN cargo build --release -p steam-fan-server --locked

FROM debian:bookworm-slim
# downloads: certificates, MEGA (megatools); unpacking: bsdtar, 7z (rar, 7z, tar.xz...)
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates megatools libarchive-tools p7zip-full \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/steam-fan-server /usr/local/bin/steam-fan-server
ENV SF_SHELF=/shelf SF_DATA=/data SF_PORT=8484
VOLUME ["/data"]
EXPOSE 8484
HEALTHCHECK --interval=1m --timeout=5s CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/${SF_PORT:-8484}' || exit 1
LABEL org.opencontainers.image.title="Steam-Fan-Server" org.opencontainers.image.description="The shelf as a service: catalog, downloads, updates and game files for Steam-Fan"
CMD ["steam-fan-server"]
