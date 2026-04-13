FROM rust:slim-bookworm AS build

WORKDIR /app
COPY . /app
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=build /app/target/release/mirage /

ENTRYPOINT [ "/mirage" ]
