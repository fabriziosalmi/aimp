use crate::crdt::merkle_dag::DagNode;
use crate::error::{AimpError, AimpResult};
use crate::protocol::Hash32;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use hkdf::Hkdf;
use redb::{Database, ReadableTable, TableDefinition};
use sha2::Sha256;
use std::path::Path;

/// Salt used for HKDF key derivation — domain-separates storage encryption keys.
const HKDF_SALT: &[u8] = b"aimp-store-v1";

/// Table definition for DAG nodes in redb.
const DAG_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("dag_nodes");

/// High-performance persistent storage for AIMP Merkle-DAG using redb with encryption-at-rest.
///
/// Encryption keys are derived via HKDF-SHA256 from the node's Noise static secret,
/// ensuring proper key separation between transport and storage layers.
pub struct PersistentStore {
    db: Database,
    cipher: ChaCha20Poly1305,
}

impl PersistentStore {
    /// Open or create a persistent store at the given path.
    ///
    /// The `ikm` (input key material) is the raw Noise static secret.
    /// A proper encryption key is derived via HKDF-SHA256 before use.
    pub fn open<P: AsRef<Path>>(path: P, ikm: [u8; 32]) -> AimpResult<Self> {
        let db_path = path.as_ref().join("aimp.redb");
        std::fs::create_dir_all(path.as_ref())?;
        let db = Database::create(db_path)?;

        let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &ikm);
        let mut derived_key = [0u8; 32];
        hk.expand(b"aimp-chacha20poly1305", &mut derived_key)
            .map_err(|_| AimpError::Encryption("HKDF expand failed".into()))?;

        let key = Key::from_slice(&derived_key);
        let cipher = ChaCha20Poly1305::new(key);
        Ok(Self { db, cipher })
    }

    /// Persist a DagNode using its hash as the key, with AEAD encryption.
    pub fn save_node(&self, hash: &Hash32, node: &DagNode) -> AimpResult<()> {
        let plaintext = rmp_serde::to_vec(node)?;
        let nonce = Nonce::from_slice(&hash[0..12]);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|_| AimpError::Encryption("ChaCha20Poly1305 encrypt failed".into()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DAG_TABLE)?;
            table.insert(hash.as_slice(), ciphertext.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a specific node from disk and decrypt.
    pub fn load_node(&self, hash: &Hash32) -> AimpResult<Option<DagNode>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DAG_TABLE)?;

        if let Some(value) = table.get(hash.as_slice())? {
            let ciphertext = value.value();
            let nonce = Nonce::from_slice(&hash[0..12]);
            let plaintext = self.cipher.decrypt(nonce, ciphertext).map_err(|_| {
                AimpError::Encryption("Decryption failed: invalid key or data".into())
            })?;

            let node: DagNode = rmp_serde::from_slice(&plaintext)?;
            Ok(Some(node))
        } else {
            Ok(None)
        }
    }

    /// Load stored nodes in batches, decrypting each one.
    ///
    /// Calls `callback` with each batch of `(Hash32, DagNode)` pairs.
    /// This avoids loading the entire database into a single Vec, preventing
    /// OOM on large databases.
    pub fn load_batched(
        &self,
        batch_size: usize,
        mut callback: impl FnMut(Vec<(Hash32, DagNode)>),
    ) {
        let read_txn = match self.db.begin_read() {
            Ok(t) => t,
            Err(_) => return,
        };

        let table = match read_txn.open_table(DAG_TABLE) {
            Ok(t) => t,
            Err(_) => return,
        };

        let iter = match table.iter() {
            Ok(i) => i,
            Err(_) => return,
        };

        let mut batch = Vec::with_capacity(batch_size);

        for (k, v) in iter.flatten() {
            let key_bytes = k.value();
            let val_bytes = v.value();

            if key_bytes.len() == 32 {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(key_bytes);
                let nonce = Nonce::from_slice(&hash[0..12]);

                if let Ok(plaintext) = self.cipher.decrypt(nonce, val_bytes) {
                    if let Ok(node) = rmp_serde::from_slice::<DagNode>(&plaintext) {
                        batch.push((hash, node));
                        if batch.len() >= batch_size {
                            callback(std::mem::take(&mut batch));
                            batch = Vec::with_capacity(batch_size);
                        }
                    }
                }
            }
        }

        if !batch.is_empty() {
            callback(batch);
        }
    }

    /// Convenience: load all stored nodes at once (small databases only).
    pub fn load_all(&self) -> Vec<(Hash32, DagNode)> {
        let mut results = Vec::new();
        self.load_batched(4096, |batch| results.extend(batch));
        results
    }

    /// Flush pending writes (no-op for redb — writes are committed immediately).
    pub fn flush(&self) -> AimpResult<()> {
        Ok(())
    }
}
