# OSIRIS — tek imaj (R7): `osiris-server` (api | pipeline | collector | migrate | models | …) + `osiris` CLI.
# Alt komut compose `command` ile seçilir; varsayılan REST API.
#   docker compose --profile api up -d --build
# Modeller imaja GÖMÜLMEZ (NER modeli AFL-3.0): ./models salt-okunur bağlanır; indirme:
#   docker compose --profile models run --rm osiris-models
FROM rust:1.99.1-slim-trixie AS build
# rust-toolchain.toml'daki bileşenleri (rustfmt/clippy) indirmesin: imajdaki sürüm aynı
ENV RUSTUP_TOOLCHAIN=1.99.1 \
    CARGO_TERM_COLOR=never
RUN apt-get update \
 && apt-get install -y --no-install-recommends g++ ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
# Derleme zamanında gömülenler: manifest.json + username-search site DB'leri, migration'lar
COPY plugins ./plugins
COPY db ./db
RUN cargo build --release --locked -p osiris-server -p osiris-cli \
 && mkdir -p /out/app/models \
 && cp target/release/osiris-server target/release/osiris /out/

# Çalışma imajı: kabuk ve paket yöneticisi yok; glibc + libstdc++ (ONNX Runtime statik bağlı),
# CA sertifikaları ve root olmayan kullanıcı (65532) hazır.
FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /out/osiris-server /out/osiris /usr/local/bin/
COPY --from=build --chown=65532:65532 /out/app /app
ENV OSIRIS_MODEL_DIR=/app/models
WORKDIR /app
USER nonroot
EXPOSE 8000
ENTRYPOINT ["osiris-server"]
CMD ["api", "--addr", "0.0.0.0:8000"]
