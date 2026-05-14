use std::sync::OnceLock;

/// Install jsonwebtoken's process-global crypto provider exactly once.
pub fn install_crypto_provider() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
    });
}
