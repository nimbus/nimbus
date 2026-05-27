# syntax=docker/dockerfile:1@sha256:87999aa3d42bdc6bea60565083ee17e86d1f3339802f543c0d03998580f9cb89

FROM debian:13-slim@sha256:b6e2a152f22a40ff69d92cb397223c906017e1391a73c952b588e51af8883bf8

ARG NIMBUS_ARCHIVE
ARG NIMBUS_VERSION
ARG VCS_REF
ARG BUILD_DATE
ARG SOURCE_REPOSITORY="https://github.com/nimbus/nimbus"

LABEL org.opencontainers.image.title="Nimbus" \
      org.opencontainers.image.description="Convex-compatible reactive backend server" \
      org.opencontainers.image.url="https://github.com/nimbus/nimbus" \
      org.opencontainers.image.source="${SOURCE_REPOSITORY}" \
      org.opencontainers.image.version="${NIMBUS_VERSION}" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.licenses="LicenseRef-Nimbus-Community"

RUN set -eux; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd --system --gid 10001 nimbus; \
    useradd --system --uid 10001 --gid nimbus --home-dir /var/lib/nimbus --shell /usr/sbin/nologin nimbus; \
    mkdir -p /usr/local/share/doc/nimbus /var/lib/nimbus/data /var/lib/nimbus/control; \
    chown -R nimbus:nimbus /var/lib/nimbus

COPY ${NIMBUS_ARCHIVE} /tmp/nimbus.tar.gz

RUN set -eux; \
    tar -xzf /tmp/nimbus.tar.gz -C /tmp nimbus README.md LICENSE; \
    install -m 0755 /tmp/nimbus /usr/local/bin/nimbus; \
    install -m 0644 /tmp/README.md /usr/local/share/doc/nimbus/README.md; \
    install -m 0644 /tmp/LICENSE /usr/local/share/doc/nimbus/LICENSE; \
    rm -f /tmp/nimbus.tar.gz /tmp/nimbus /tmp/README.md /tmp/LICENSE

ENV HOME=/var/lib/nimbus \
    NIMBUS_CONTAINER=oci

WORKDIR /var/lib/nimbus
USER 10001:10001
EXPOSE 8080
VOLUME ["/var/lib/nimbus"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["curl", "-fsS", "http://127.0.0.1:8080/health"]

ENTRYPOINT ["nimbus"]
CMD ["start", "--host", "0.0.0.0", "--allow-network", "--data-dir", "/var/lib/nimbus/data", "--control-data-dir", "/var/lib/nimbus/control"]
