//! hellodb Cryptographic Primitives
//!
//! Provides Ed25519 signing, X25519 ECDH (Curve25519), ChaCha20-Poly1305 AEAD
//! encryption, BLAKE3 content hashing, and hierarchical key derivation
//! for the hellodb sovereign data layer.

pub mod encryption;
pub mod error;
pub mod hash;
pub mod identity;
pub mod keychain;

pub use encryption::{open, seal, DecryptionKey, EncryptionKey, SealedBox, SharedSecret};
pub use error::CryptoError;
pub use hash::{content_hash, content_hash_bytes};
pub use identity::{KeyPair, Signature, SigningKey, VerifyingKey};
pub use keychain::{MasterKey, NamespaceKey};
