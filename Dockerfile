FROM rust:1.80-alpine AS builder
RUN apk add --no-cache build-base
WORKDIR /usr/src/asterconf
COPY . .
RUN cargo build --release
CMD ["asterconf"]

FROM alpine:latest
# tz should be set in docker compose or similar
RUN apk add --no-cache tzdata
WORKDIR /asterconf
COPY --from=builder /usr/src/asterconf/target/release/asterconf ./
CMD ["./asterconf"]

