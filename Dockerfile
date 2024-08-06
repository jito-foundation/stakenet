FROM rust:1.75-slim-buster as builder

RUN apt-get update && apt-get install -y libudev-dev clang pkg-config libssl-dev build-essential cmake protobuf-compiler

RUN update-ca-certificates

WORKDIR /usr/src/app

COPY . .

RUN --mount=type=cache,mode=0777,target=/home/root/app/target \
    --mount=type=cache,mode=0777,target=/usr/local/cargo/registry \
	cargo build --release --bin validator-keeper

#########

FROM rust:1.75-slim-buster as metrics-builder
RUN apt-get update && apt-get install -y libudev-dev clang pkg-config libssl-dev build-essential cmake protobuf-compiler
RUN update-ca-certificates
WORKDIR /usr/src/app
COPY . .
RUN --mount=type=cache,mode=0777,target=/home/root/app/target \
    --mount=type=cache,mode=0777,target=/usr/local/cargo/registry \
    cargo build --release --bin stakenet-metrics-service

#########

FROM debian:buster as validator-history
RUN apt-get update && apt-get install -y ca-certificates
ENV APP="validator-keeper"

COPY --from=builder /usr/src/app/target/release/$APP ./$APP

ENTRYPOINT ./$APP
 

#########

FROM debian:buster as stakenet-metrics
RUN apt-get update && apt-get install -y ca-certificates
ENV APP="stakenet-metrics-service"
COPY --from=metrics-builder /usr/src/app/target/release/$APP ./$APP
ENTRYPOINT ./$APP