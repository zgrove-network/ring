# Built here rather than on the server: the box has two gigabytes of memory
# and linking this pulls in the whole Zcash stack.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY wallet/Cargo.toml wallet/Cargo.lock ./
# A throwaway main, so the dependency build is cached separately from ours.
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY wallet/src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Not root. This process holds a viewing key, which reads every position.
RUN useradd --uid 10001 --create-home --home-dir /home/ring ring
COPY --from=build /src/target/release/ring-wallet /usr/local/bin/ring-wallet
USER ring
WORKDIR /home/ring
ENTRYPOINT ["ring-wallet"]
