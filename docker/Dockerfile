# =============================================================================
# dwg-mcp-server image
#
# Three stages:
#   1. libredwg-prebuilt — unpack a Git LFS LibreDWG bundle
#   2. worker-builder    — compile dwg-worker (Rust) against prebuilt LibreDWG
#   3. runtime           — Python MCP server + static-linked worker binary
#
# Runtime copies use COPY --chown so we never chown huge trees (saves image size).
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: LibreDWG prebuilt bundle -> /opt/libredwg
# -----------------------------------------------------------------------------
FROM rust:1-bookworm AS libredwg-prebuilt

WORKDIR /build
ARG LIBREDWG_TARGET=x86_64-unknown-linux-gnu
COPY prebuilt/libredwg/${LIBREDWG_TARGET}.tar.gz /tmp/libredwg.tar.gz
RUN mkdir -p /opt/libredwg \
    && tar -xzf /tmp/libredwg.tar.gz -C /opt/libredwg \
    && test -f /opt/libredwg/src/.libs/libredwg.a \
    && test -f /opt/libredwg/src/config.h \
    && test -f /opt/libredwg/src/classes.c \
    && test -f /opt/libredwg/src/dynapi.c \
    && test -f /opt/libredwg/include/dwg_api.h

# -----------------------------------------------------------------------------
# Stage 2: dwg-worker binary (Rust)
# -----------------------------------------------------------------------------
FROM rust:1-bookworm AS worker-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        clang \
        libclang-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=libredwg-prebuilt /opt/libredwg /opt/libredwg

ENV LIBREDWG_ROOT_DIR=/opt/libredwg
ENV LIBREDWG_SOURCE_ROOT=/opt/libredwg/src

RUN cargo build -p dwg-worker --release \
    && strip /app/target/release/dwg-worker

# -----------------------------------------------------------------------------
# Stage 3: Final image — Python + app + native pieces
# -----------------------------------------------------------------------------
FROM python:3.13-slim AS runtime

LABEL io.modelcontextprotocol.server.name="io.github.dimitrovakulenko/dwg-mcp-server"

ENV PIP_NO_CACHE_DIR=1

RUN useradd --create-home --shell /bin/bash appuser \
    && mkdir -p /app/bin /app/third_party/libredwg/src \
    && chown -R appuser:appuser /app

# Two C files are used at runtime for schema extraction.
COPY --chown=appuser:appuser --from=libredwg-prebuilt \
    /opt/libredwg/src/classes.c \
    /opt/libredwg/src/dynapi.c \
    /app/third_party/libredwg/src/

# Rust worker links LibreDWG statically at build time.
COPY --chown=appuser:appuser --from=worker-builder /app/target/release/dwg-worker /app/bin/dwg-worker

# MCP server source (installed into site-packages by pip below)
COPY --chown=appuser:appuser server /app/server

WORKDIR /app

RUN python -m pip install --no-cache-dir /app/server \
    && rm -rf /root/.cache/pip

ENV DWG_WORKER_BIN=/app/bin/dwg-worker
ENV LIBREDWG_SOURCE_ROOT=/app/third_party/libredwg/src
ENV PYTHONPATH=/app/server/src

USER appuser
ENTRYPOINT ["python3", "-m", "dwg_mcp_server"]
