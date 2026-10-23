# Coxswain's Stockfish image: Stockfish 17.1 built from its released source (checked by its
# digest) with its two evaluation networks inside (checked by their digests, as its own
# scripts/net.sh does), linked statically, alone in an otherwise empty image. Built for amd64
# and arm64 by .github/workflows/stockfish-image.yml and pushed to
# ghcr.io/mwo-dk/coxswain-stockfish. Coxswain runs it as `[preview.images] stockfish` for the
# chess view's engine when no Stockfish is installed (crates/coxswain-core/src/chess/play.rs),
# without network, read-only, with no capabilities, talking UCI over stdin and stdout.
#
# The image holds no shell and no package manager: only /stockfish. Stockfish is GPL-3.0; its
# source is the release named here, unchanged.

FROM docker.io/library/alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6 AS build
ARG TARGETARCH
RUN apk add --no-cache build-base curl
WORKDIR /src
RUN curl -fsSLo sf.tar.gz https://github.com/official-stockfish/Stockfish/archive/refs/tags/sf_17.1.tar.gz \
 && echo "0cfd9396438798cc68f5c0d5fa0bb458bb8ffff7de06add841aaeace86bec1f1  sf.tar.gz" | sha256sum -c - \
 && tar xzf sf.tar.gz --strip-components=1 && rm sf.tar.gz
# A build that runs on any CPU of its kind: SSE4.1 and POPCNT on amd64 (every one since about
# 2008), plain ARMv8 on arm64. Alpine's fortified headers do not go with Stockfish's link-time
# optimisation, so they are left out (-U_FORTIFY_SOURCE).
RUN cd src \
 && arch=$([ "$TARGETARCH" = arm64 ] && echo armv8 || echo x86-64-sse41-popcnt) \
 && make -j"$(nproc)" build ARCH="$arch" COMP=gcc EXTRALDFLAGS=-static EXTRACXXFLAGS=-U_FORTIFY_SOURCE \
 && strip stockfish \
 && ./stockfish bench 16 1 6 >/dev/null 2>&1

FROM scratch
COPY --from=build /src/src/stockfish /stockfish
COPY --from=build /src/Copying.txt /COPYING
LABEL org.opencontainers.image.title="coxswain-stockfish" \
      org.opencontainers.image.description="Stockfish 17.1, static, for Coxswain's chess engine when none is installed" \
      org.opencontainers.image.source="https://github.com/mwo-dk/coxswain" \
      org.opencontainers.image.licenses="GPL-3.0-only"
USER 65534:65534
ENTRYPOINT ["/stockfish"]
