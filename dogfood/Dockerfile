FROM rust:1.92.0-bookworm

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        clang \
        cmake \
        curl \
        docker.io \
        file \
        git \
        gzip \
        jq \
        less \
        lsof \
        make \
        ninja-build \
        pkg-config \
        procps \
        protobuf-compiler \
        python3 \
        rsync \
        tar \
        unzip \
        xz-utils \
        build-essential \
        libclang-dev \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

ENV DOCKER_DEFAULT_PLATFORM=linux/amd64
ENV RUST_BACKTRACE=1

WORKDIR /dogfood

CMD ["/bin/bash", "-l"]
