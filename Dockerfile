# CSTL : le serveur de controle et les commandes du SDK Python, dans une image.
#
# Construite et verifiee le 2026-11-27 : image de 246 Mo, serveur demarre,
# `docker exec cstl cstl-demo` -> "Tout est conforme : 7/7."
#
#   docker build -t cstl .
#   docker run -d --name cstl -p 5050:5050 -v cstl-data:/data \
#       -e CSTL_POLICY_AUTHORS=owner_ops cstl
#   docker exec cstl cat /data/cstl_adn.db.server_key.pub   # cle publique du serveur, pour les gardes
#   docker exec -it cstl cstl-demo                          # la demonstration, 7 verifications
#
# - Les donnees (base, cle du serveur, audit) sont dans /data : gardez ce volume.
# - Le serveur ecoute sur 0.0.0.0:5050 DANS le conteneur ; les signatures sont
#   obligatoires. Hors d'une machine de confiance, ajoutez TLS (CSTL_TLS_*).
# - L'API REST (tableau de bord, entree JSON) n'ecoute que sur 127.0.0.1 : avec
#   --network host sous Linux, ou depuis le conteneur (docker exec).

# Image Rust complete : OpenSSL et pkg-config y sont deja (aucun apt-get, donc
# aucun acces HTTP non chiffre : l'image se construit aussi derriere un proxy strict).
FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin cstl_parser && strip target/release/cstl_parser

# python:slim contient deja les certificats et libssl dont le serveur a besoin.
FROM python:3.13-slim-bookworm
COPY --from=build /src/target/release/cstl_parser /usr/local/bin/cstl_parser
COPY sdk/python /opt/cstl-sdk
RUN pip install --no-cache-dir /opt/cstl-sdk && mkdir -p /root/.cstl
# cstl-demo cherche le serveur dans le depot : on lui donne le binaire installe.
ENV CSTL_BINARY=/usr/local/bin/cstl_parser \
    CSTL_BIND_HOST=0.0.0.0 \
    CSTL_PORT=5050 \
    CSTL_KB_VERIFY=off
WORKDIR /data
VOLUME ["/data"]
EXPOSE 5050
ENTRYPOINT ["cstl_parser"]
