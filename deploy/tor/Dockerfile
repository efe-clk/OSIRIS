# OSIRIS — Tor SOCKS5 proxy (yalnızca istemci; relay/çıkış düğümü DEĞİL)
# Kullanım: docker compose --profile darknet up -d tor
FROM alpine:3.24.2

RUN apk add --no-cache tor \
 && mkdir -p /var/lib/tor && chown -R tor /var/lib/tor && chmod 700 /var/lib/tor

COPY torrc /etc/tor/torrc

USER tor
EXPOSE 9050

HEALTHCHECK --interval=60s --timeout=10s --start-period=120s --retries=3 \
  CMD nc -z 127.0.0.1 9050 || exit 1

CMD ["tor", "-f", "/etc/tor/torrc"]
