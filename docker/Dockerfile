# =============================================================================
# dwg-mcp-server image
#
# Three stages:
#   1. libredwg-builder  — compile LibreDWG; shared libs under /opt/libredwg
#   2. worker-builder    — compile dwg-worker (Rust) against vendored LibreDWG
#   3. runtime           — Python MCP server + worker binary + libredwg.so only
#
# Runtime copies use COPY --chown so we never chown huge trees (saves image size).
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: LibreDWG (C autotools) → /opt/libredwg
# -----------------------------------------------------------------------------
FROM rust:1-bookworm AS libredwg-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        autoconf \
        automake \
        bison \
        build-essential \
        flex \
        libtool \
        make \
        texinfo \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY third_party/libredwg /build/libredwg

RUN cd /build/libredwg \
    && perl -0pi -e 's/\nif command -v git >\/dev\/null\nthen\n    git submodule update --init --recursive\nelse\n    curl https:\/\/raw\.githubusercontent\.com\/zserge\/jsmn\/master\/jsmn\.h -o jsmn\/jsmn\.h\nfi/\n/' autogen.sh \
    && libtoolize -i \
    && ./autogen.sh \
    && ./configure --prefix=/opt/libredwg \
    && make -j"$(nproc)" \
    && make install \
    && find /opt/libredwg -name '*.a' -delete

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
COPY --from=libredwg-builder /build/libredwg /app/third_party/libredwg

RUN cargo build -p dwg-worker --release \
    && strip /app/target/release/dwg-worker

# -----------------------------------------------------------------------------
# Stage 3: Final image — Python + app + native pieces
# -----------------------------------------------------------------------------
FROM python:3.13-slim AS runtime

ENV PIP_NO_CACHE_DIR=1

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libxml2 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash appuser \
    && mkdir -p /opt/libredwg/lib /app/bin /app/third_party/libredwg/src \
    && chown -R appuser:appuser /app /opt/libredwg

# libredwg.so (runtime) + two C files (schema for dwg-libredwg)
COPY --chown=appuser:appuser --from=libredwg-builder /opt/libredwg/lib /opt/libredwg/lib
COPY --chown=appuser:appuser --from=libredwg-builder \
    /build/libredwg/src/classes.c \
    /build/libredwg/src/dynapi.c \
    /app/third_party/libredwg/src/

# Rust worker (links libredwg at build time; loads libredwg.so at runtime via LD_LIBRARY_PATH)
COPY --chown=appuser:appuser --from=worker-builder /app/target/release/dwg-worker /app/bin/dwg-worker

# MCP server source (installed into site-packages by pip below)
COPY --chown=appuser:appuser server /app/server

WORKDIR /app

RUN python -m pip install --no-cache-dir /app/server \
    && rm -rf /root/.cache/pip

ENV DWG_WORKER_BIN=/app/bin/dwg-worker
ENV LD_LIBRARY_PATH=/opt/libredwg/lib
ENV LIBREDWG_SOURCE_ROOT=/app/third_party/libredwg/src
ENV PYTHONPATH=/app/server/src

USER appuser
ENTRYPOINT ["python3", "-m", "dwg_mcp_server"]
