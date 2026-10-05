//! Shared HTTP setup.

use std::sync::Once;

/// reqwest is built without a default TLS crypto provider; install ring once per process.
pub fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
