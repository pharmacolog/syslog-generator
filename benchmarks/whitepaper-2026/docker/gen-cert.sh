#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERT_DIR="${SCRIPT_DIR}/certs"
CERT_FILE="${CERT_DIR}/server.pem"
KEY_FILE="${CERT_DIR}/server.key"

mkdir -p "${CERT_DIR}"

if ! command -v openssl >/dev/null 2>&1; then
    echo "ERROR: openssl not found" >&2
    exit 1
fi

openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "${KEY_FILE}" \
    -out "${CERT_FILE}" \
    -days 365 \
    -subj "/CN=localhost" \
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
    2>/dev/null

chmod 600 "${KEY_FILE}"
chmod 644 "${CERT_FILE}"

echo "TLS cert generated:"
echo "  cert: ${CERT_FILE}"
echo "  key:  ${KEY_FILE}"
echo
echo "Validating (should print 'verify OK'):"
openssl verify -CAfile "${CERT_FILE}" "${CERT_FILE}"
