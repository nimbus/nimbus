
import Module, { getBuiltinModule as getNimbusBuiltinModule } from "node:nimbus/module";
import { ERR_TLS_INVALID_CONTEXT } from "ext:deno_node/internal/errors.ts";

const tlsBuiltin = getNimbusBuiltinModule?.("tls") ?? Module?.getBuiltinModule?.("tls");
if (!tlsBuiltin || typeof tlsBuiltin.connect !== "function") {
  throw new Error(
    "Nimbus Node22 bootstrap expected node:nimbus/module to expose the tls builtin",
  );
}

const {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  CryptoStream,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecurePair,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createServer,
  getCiphers,
  rootCertificates,
  setDefaultCACertificates,
} = tlsBuiltin;

function createSecurePair(context, ...args) {
  if (!context || typeof context !== "object" || !("context" in context)) {
    throw new ERR_TLS_INVALID_CONTEXT("context");
  }
  return tlsBuiltin.createSecurePair(context, ...args);
}

const defaultExport = Object.create(tlsBuiltin);
Object.assign(defaultExport, {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  CryptoStream,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecurePair,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createSecurePair,
  createServer,
  getCiphers,
  rootCertificates,
  setDefaultCACertificates,
});

export {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  CryptoStream,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecurePair,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createSecurePair,
  createServer,
  getCiphers,
  rootCertificates,
  setDefaultCACertificates,
};
export default defaultExport;
