# syntax=docker/dockerfile:1
#
# The release build environment (scripts/build-environment.json): Ubuntu 24.04, x86_64,
# with every input that decides what the packages contain pinned. Release artifacts are
# built in it on any Linux host with Docker or Podman; see docs/release/RELEASE.md.
#
#   docker build -f scripts/release.Containerfile -t wyss-console-release scripts
#   docker run --rm -v "$PWD":/build/wyss-console wyss-console-release
#
# The checkout is mounted at /build/wyss-console, the path the build maps its sources to
# anyway, and the artifacts are written into it (.artifacts/release/). Nothing secret is
# put in the image. Behind a TLS-intercepting proxy, its CA is handed to the build as the
# secret `extra-ca` (it is not kept in the image) and, at run time, mounted and named in
# the tools' own variables (CURL_CA_BUNDLE, NODE_EXTRA_CA_CERTS, CARGO_HTTP_CAINFO).

# Ubuntu 24.04 (noble), by digest: the tag moves, the digest does not.
FROM docker.io/library/ubuntu:24.04@sha256:534baea6a22c03a63003dbc8dbe78fe34bc0d7e595d9a9dc9834884ff530eb55

# Every distribution package as it was in the archive at this moment
# (snapshot.ubuntu.com). The AppImage carries these libraries; the reviewed libgnutls
# keys (scripts/reviewed-library-keys.json) were taken from libgnutls30t64 3.8.3-1.1ubuntu3.6.
ARG UBUNTU_SNAPSHOT=20261011T220000Z
ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 TZ=UTC

# ca-certificates comes first, from the image's own archive, because the snapshot is only
# served over https; everything after it is from the snapshot.
RUN --mount=type=secret,id=extra-ca,required=false \
    apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && if [ -s /run/secrets/extra-ca ]; then \
      cp /run/secrets/extra-ca /usr/local/share/ca-certificates/extra-ca.crt \
      && update-ca-certificates; \
    fi \
 && apt-get update --snapshot "$UBUNTU_SNAPSHOT" \
 && apt-get install -y --no-install-recommends --snapshot "$UBUNTU_SNAPSHOT" \
      build-essential curl desktop-file-utils file git patchelf squashfs-tools xz-utils \
      libayatana-appindicator3-dev libgtk-3-dev librsvg2-dev libssl-dev \
      libwebkit2gtk-4.1-dev libxdo-dev \
 && rm -f /usr/local/share/ca-certificates/extra-ca.crt \
 && update-ca-certificates --fresh \
 && rm -rf /var/lib/apt/lists/*

# Node and npm, the versions package.json declares (engines: node >= 26, npm >= 12), against
# the digests nodejs.org publishes for Node (SHASUMS256.txt) and the registry for npm (its
# sha512 integrity, of which this is the SHA-256 of the same file). npm refuses, rather than
# warns about, any dependency whose declared engines this toolchain does not meet.
ARG NODE_VERSION=26.11.1
ARG NODE_SHA256=3883bfc73f9a680ca4eab04b196068aaaab1373ffa77d8fc1a4408222495b651
ARG NPM_VERSION=12.2.0
ARG NPM_SHA256=6666b48816b39b86c3febac7b51a4ee4de6c5ca589c382ad8004b6b113f86677
RUN --mount=type=secret,id=extra-ca,required=false \
    if [ -s /run/secrets/extra-ca ]; then export CURL_CA_BUNDLE=/run/secrets/extra-ca; fi \
 && curl -fsSLo /tmp/node.tar.xz \
      "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
 && echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum -c - \
 && mkdir /opt/node \
 && tar -xJf /tmp/node.tar.xz -C /opt/node --strip-components=1 \
 && rm /tmp/node.tar.xz \
 && curl -fsSLo /tmp/npm.tgz "https://registry.npmjs.org/npm/-/npm-${NPM_VERSION}.tgz" \
 && echo "${NPM_SHA256}  /tmp/npm.tgz" | sha256sum -c - \
 && export PATH=/opt/node/bin:$PATH \
 && npm install --global --offline --no-audit --no-fund /tmp/npm.tgz \
 && rm /tmp/npm.tgz \
 && test "$(npm --version)" = "${NPM_VERSION}"
ENV npm_config_engine_strict=true

# Rust, through a rustup-init of a fixed version and digest; the toolchain is a fixed
# release, which rustup checks against the digests of its own channel manifest.
ARG RUSTUP_VERSION=1.28.2
ARG RUSTUP_SHA256=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c
ARG RUST_VERSION=1.97.0
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo
RUN --mount=type=secret,id=extra-ca,required=false \
    if [ -s /run/secrets/extra-ca ]; then export CURL_CA_BUNDLE=/run/secrets/extra-ca; fi \
 && curl -fsSLo /tmp/rustup-init \
      "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
 && echo "${RUSTUP_SHA256}  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal \
      --default-toolchain "${RUST_VERSION}" \
 && rm /tmp/rustup-init

ENV PATH=/opt/cargo/bin:/opt/node/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
# The checkout belongs to whoever owns it on the host.
RUN git config --system --add safe.directory /build/wyss-console
WORKDIR /build/wyss-console

# The same commands as on any host: install the locked dependencies, then build, verify,
# and keep the artifacts only if they verify.
CMD ["sh", "-c", "npm ci && npm run release:package"]
