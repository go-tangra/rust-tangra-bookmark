# ---- Stage 1: Build frontend module ----
FROM node:20-alpine AS frontend-builder
RUN npm install -g pnpm@9
WORKDIR /frontend
COPY frontend/package.json frontend/pnpm-lock.yaml* ./
RUN pnpm install --frozen-lockfile || pnpm install
COPY frontend/ .
RUN pnpm build

# ---- Stage 2: Build Rust ----
FROM rust:1-alpine AS rust-builder

RUN apk add --no-cache musl-dev curl unzip && \
    curl -LO https://github.com/protocolbuffers/protobuf/releases/download/v28.3/protoc-28.3-linux-x86_64.zip && \
    unzip protoc-28.3-linux-x86_64.zip -d /usr/local && \
    rm protoc-28.3-linux-x86_64.zip && \
    protoc --version

ENV PROTOC=/usr/local/bin/protoc
ENV PROTOC_INCLUDE=/usr/local/include

WORKDIR /build

# Cache dependencies
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    mkdir -p proto/bookmark/service/v1 proto/common/service/v1 && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src proto

# Copy source and build
COPY . .
RUN cargo build --release

# ---- Stage 3: Runtime ----
FROM alpine:3.20

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=rust-builder /build/target/release/bookmark-server /app/bookmark-server
COPY --from=rust-builder /build/configs /app/configs
COPY --from=rust-builder /build/assets /app/assets
COPY --from=rust-builder /build/migrations /app/migrations
COPY --from=frontend-builder /frontend/dist /app/frontend-dist

EXPOSE 9700 9701

ENTRYPOINT ["/app/bookmark-server"]
