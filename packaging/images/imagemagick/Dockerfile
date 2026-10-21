# Coxswain's ImageMagick image: Alpine's ImageMagick 7 with HEIC and AVIF (libheif with libde265
# and dav1d), camera RAW (LibRaw), JPEG, JPEG XL, WebP and TIFF, and Coxswain's policy baked in.
# Built for amd64 and arm64 by .github/workflows/imagemagick-image.yml, weekly for Alpine's
# security updates, and pushed to ghcr.io/mwo-dk/coxswain-imagemagick; Coxswain pins it by
# digest (crates/coxswain-core/src/images.rs).
#
# The image holds no shell and no package manager: only magick, its libraries, its policy and
# the list of the Alpine packages it was made of (/lib/apk/db/installed: versions, licences).
# It runs as nobody unless told otherwise (Coxswain passes --user).

FROM docker.io/library/alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6 AS build
RUN apk upgrade --no-cache \
 && apk add --no-cache imagemagick imagemagick-heic imagemagick-jpeg imagemagick-jxl imagemagick-raw \
      imagemagick-tiff imagemagick-webp libheif-libde265 libheif-dav1d
# The same policy as media.rs writes for an installed ImageMagick (a test keeps them equal):
# no delegate programs, no coder but the pictures Coxswain reads and writes, bounded resources.
COPY policy.xml /etc/ImageMagick-7/policy.xml
RUN chmod 0444 /etc/ImageMagick-7/policy.xml \
 && rm -rf /sbin/apk /etc/apk /usr/share/apk /var/cache/apk /usr/share/doc /usr/share/man \
 && /bin/busybox --list-full | sed 's|^|/|' | xargs rm -f \
 && /bin/busybox rm /bin/busybox

# One layer, without what was taken out above.
FROM scratch
COPY --from=build / /
LABEL org.opencontainers.image.title="coxswain-imagemagick" \
      org.opencontainers.image.description="ImageMagick 7 from Alpine with HEIC, AVIF, RAW, JPEG XL, WebP and TIFF, for Coxswain's picture previews and conversions" \
      org.opencontainers.image.source="https://github.com/mwo-dk/coxswain" \
      org.opencontainers.image.licenses="ImageMagick AND GPL-2.0-or-later AND LGPL-3.0-or-later AND (CDDL-1.0 OR LGPL-2.1-only) AND Apache-2.0 AND BSD-3-Clause AND BSD-2-Clause AND MIT"
USER 65534:65534
WORKDIR /tmp
ENTRYPOINT ["magick"]
