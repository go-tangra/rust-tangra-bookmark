# ---- Builder ----
FROM rust:1-alpine AS builder

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

# ---- Runtime ----
FROM alpine:3.20

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /build/target/release/bookmark-server /app/bookmark-server
COPY --from=builder /build/configs /app/configs
COPY --from=builder /build/assets /app/assets
COPY --from=builder /build/migrations /app/migrations

EXPOSE 9700

ENTRYPOINT ["/app/bookmark-server"]
