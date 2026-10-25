# Coxswain's DuckDB image: DuckDB's own released command line (2.0.0, its musl build for Linux,
# checked by its digest), unchanged, with the few libraries it needs from Alpine (musl,
# libstdc++, libgcc), in an otherwise empty image. DuckDB publishes no image of its own. Built
# for amd64 and arm64 by .github/workflows/duckdb-image.yml and pushed to
# ghcr.io/mwo-dk/coxswain-duckdb. Coxswain runs it as `[preview.images] duckdb` for the tables
# of .duckdb files (gui/src-tauri/src/convert.rs) when no duckdb is installed: without network,
# read-only, with no capabilities, as `duckdb -readonly -json <file> -c <query>`.
#
# The image holds no shell and no package manager: only /duckdb and its libraries. DuckDB is
# MIT (/licenses/duckdb); musl MIT; libstdc++ and libgcc GPL-3.0-or-later WITH
# GCC-exception-3.1 (Alpine's packages, named with their versions and licences in
# /licenses/alpine-packages).

FROM docker.io/library/alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6 AS build
ARG TARGETARCH
RUN apk add --no-cache curl libstdc++
WORKDIR /out
RUN case "$TARGETARCH" in \
      amd64) sum=3285e4083a79955b0606ab799ebc1493087307cdc76b4827308b1a4dcdb371c9 ;; \
      arm64) sum=59aee3049c335917307f5df1b3722af1335fef99199dc89e8064d1b2838620c7 ;; \
      *) echo "no DuckDB for $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && curl -fsSLo /tmp/duckdb.tar.gz "https://github.com/duckdb/duckdb/releases/download/v2.0.0/duckdb-cli-linux-$TARGETARCH-musl.tar.gz" \
 && echo "$sum  /tmp/duckdb.tar.gz" | sha256sum -c - \
 && tar xzf /tmp/duckdb.tar.gz -C /out duckdb \
 && curl -fsSLo /tmp/LICENSE https://raw.githubusercontent.com/duckdb/duckdb/v2.0.0/LICENSE \
 && echo "075c33400ffcb0c586dd106a029d3e733e9da3693f8fcb9cebfc651c8a2f14e3  /tmp/LICENSE" | sha256sum -c - \
 && mkdir -p /out/lib /out/usr/lib /out/licenses /out/tmp \
 && cp /tmp/LICENSE /out/licenses/duckdb \
 && cp -L /lib/ld-musl-*.so.1 /out/lib/ \
 && for l in /lib/ld-musl-*.so.1; do ln -s "$(basename "$l")" "/out/lib/libc.musl-$(uname -m).so.1"; done \
 && cp -L /usr/lib/libstdc++.so.6 /usr/lib/libgcc_s.so.1 /out/usr/lib/ \
 && cp /lib/apk/db/installed /out/licenses/alpine-packages \
 && /out/duckdb -c "SELECT 42" | grep -q 42

FROM scratch
COPY --from=build /out/ /
LABEL org.opencontainers.image.title="coxswain-duckdb" \
      org.opencontainers.image.description="DuckDB 2.0.0's released command line, for Coxswain's previews of .duckdb files when none is installed" \
      org.opencontainers.image.source="https://github.com/mwo-dk/coxswain" \
      org.opencontainers.image.licenses="MIT AND GPL-3.0-or-later WITH GCC-exception-3.1"
ENV HOME=/tmp
USER 65534:65534
ENTRYPOINT ["/duckdb"]
