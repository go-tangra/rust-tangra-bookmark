# syntax=docker/dockerfile:1
# tangra-bookmark (go-tangra v4 bookmark module, Rust SDK): standalone image
# in the shape of the SDK's examples/notes/Dockerfile.
#
#   docker buildx build --secret id=npm_token,env=NODE_AUTH_TOKEN \
#     --build-context sdk=../go-tangra-sdk-rust \
#     --build-arg APP_VERSION=4.0.0 --build-arg VCS_REF=$(git rev-parse HEAD) -t tangra-bookmark:dev .
#
# npm_token is a GitHub token with read:packages for @go-tangra/ui on
# npm.pkg.github.com. It is mounted only for the npm ci step and written to a
# tmpfs, so it never lands in a layer.
#
# sdk: while Cargo.toml depends on the SDK by path
# (../go-tangra-sdk-rust/crates/tangra-sdk), the SDK checkout is passed as the
# named build context `sdk` (specs/038-bookmark-v4/tasks.md T090). Once the
# dependency is a git rev, drop --build-context: the empty default stage below
# is copied and cargo fetches the SDK itself.

FROM scratch AS sdk

FROM node:22-alpine AS ui
WORKDIR /src/ui
COPY ui/package.json ui/package-lock.json ./
RUN --mount=type=secret,id=npm_token,required=true \
    --mount=type=tmpfs,target=/run/npmrc \
    --mount=type=cache,target=/root/.npm \
    set -eu; \
    printf '@go-tangra:registry=https://npm.pkg.github.com\n//npm.pkg.github.com/:_authToken=%s\nignore-scripts=true\nfund=false\naudit=false\n' \
      "$(cat /run/secrets/npm_token)" > /run/npmrc/.npmrc; \
    NPM_CONFIG_USERCONFIG=/run/npmrc/.npmrc npm ci --no-audit --no-fund
COPY ui/ ./
RUN npm run build

# glibc build on Debian 12, run on Debian 12 slim (same glibc). protoc is
# vendored by the SDK build (protoc-bin-vendored); ring needs only the C
# compiler the image has.
FROM rust:1-bookworm AS build
WORKDIR /src/bookmark
COPY --from=sdk / /src/go-tangra-sdk-rust/
COPY . .
COPY --from=ui /src/ui/dist ./ui/dist
ARG APP_VERSION=dev
# Stamped into the binary and reported to the gateway (build.rs).
ENV BOOKMARK_VERSION=${APP_VERSION}
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/bookmark/target \
    cargo build --locked --release --features ui --bin bookmarksvc --bin bookmark-migrate \
 && install -D -m 0755 target/release/bookmarksvc /out/bookmarksvc \
 && install -D -m 0755 target/release/bookmark-migrate /out/bookmark-migrate

FROM debian:bookworm-slim
ARG APP_VERSION=dev
ARG VCS_REF=unknown
LABEL org.opencontainers.image.source="https://github.com/go-tangra/rust-tangra-bookmark" \
      org.opencontainers.image.title="tangra-bookmark" \
      org.opencontainers.image.description="go-tangra v4 bookmark module: tenant-scoped web bookmarks with sharing" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${APP_VERSION}" \
      org.opencontainers.image.revision="${VCS_REF}"
# ca-certificates: public-roots TLS (first enrolment through the gateway edge).
# /state is owned by app so a named volume mounted there (join bundle) is
# writable. No wget/curl: the compose healthcheck runs
# `bookmarksvc --healthcheck`. bookmark-migrate is the one-off v3 importer
# (docs/help/import-v3.md).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --uid 10001 --user-group --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin app \
 && install -d -o app -g app -m 0700 /state
COPY --from=build /out/bookmarksvc /out/bookmark-migrate /usr/local/bin/
WORKDIR /app
USER 10001:10001
ENTRYPOINT ["bookmarksvc"]
CMD ["-config","/app/config.yaml"]
