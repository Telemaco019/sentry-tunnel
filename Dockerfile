####################################################################################################
## Builder
####################################################################################################
FROM rust:1.73 AS builder

RUN rustup target add x86_64-unknown-linux-musl
RUN apt update && apt install -y musl-tools musl-dev libssl-dev pkg-config curl g++
RUN update-ca-certificates

# Create appuser
ENV USER=sentry_tunnel
ENV UID=10001

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"

# Create an empty application to cache dependency build
RUN cargo new /sentry_tunnel --bin 
WORKDIR /sentry_tunnel
RUN touch src/lib.rs
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --target x86_64-unknown-linux-musl --release

# Dependency have been built, remove the empty app and copy real source
RUN rm src/*.rs
COPY ./src ./src
RUN touch src/main.rs
RUN touch src/lib.rs
RUN rm -f ./target/release/deps/sentry_tunnel*
RUN rm -f ./target/release/deps/libsentry_tunnel*
RUN cargo build --target x86_64-unknown-linux-musl --release


#===========================#
# Install ssl certificates  #
#===========================#
FROM alpine:3.14 as alpine
RUN apk add -U --no-cache ca-certificates

# Add non-priviledged user.
RUN adduser -H -D -u 1000 sentry_tunnel

# ==========================#
# Final image				#
# ==========================#
FROM scratch

# Import from builder.
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

WORKDIR /sentry_tunnel

# Copy our build
COPY --from=builder /sentry_tunnel/target/x86_64-unknown-linux-musl/release/sentry_tunnel ./
COPY --from=alpine /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy non-priviledged user
COPY --from=0 /etc/passwd /etc/passwd

# Set non-priviledged user
USER 1001

CMD ["/sentry_tunnel/sentry_tunnel"]
