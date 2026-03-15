use sled::Db;
use crate::protocol::Hash32;
use crate::crdt::merkle_dag::DagNode;
use std::path::Path;
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, aead::{Aead, KeyInit}};

/// High-performance persistent storage for AIMP Merkle-DAG using Sled with encryption-at-rest.
pub struct PersistentStore {
    db: Db,
    cipher: ChaCha20Poly1305,
}

impl PersistentStore {
    pub fn open<P: AsRef<Path>>(path: P, encryption_key: [u8; 32]) -> Result<Self, Box<dyn std::error::Error>> {
        let db = sled::open(path)?;
        let key = Key::from_slice(&encryption_key);
        let cipher = ChaCha20Poly1305::new(key);
        Ok(Self { db, cipher })
    }

    /// Persist a DagNode using its hash as the key, with AEAD encryption.
    pub fn save_node(&self, hash: &Hash32, node: &DagNode) -> Result<(), Box<dyn std::error::Error>> {
        let plaintext = rmp_serde::to_vec(node)?;
        
        // Use first 12 bytes of deterministic hash as nonce
        let nonce = Nonce::from_slice(&hash[0..12]);
        
        let ciphertext = self.cipher.encrypt(nonce, plaintext.as_ref())
            .map_err(|_| "Encryption failure")?;

        self.db.insert(hash, ciphertext)?;
        self.db.flush()?; 
        Ok(())
    }

    /// Load a specific node from disk and decrypt.
    pub fn load_node(&self, hash: &Hash32) -> Result<Option<DagNode>, Box<dyn std::error::Error>> {
        if let Some(ciphertext) = self.db.get(hash)? {
            let nonce = Nonce::from_slice(&hash[0..12]);
            let plaintext = self.cipher.decrypt(nonce, ciphertext.as_ref())
                .map_err(|_| "Decryption failure: Invalid key or corrupted data")?;
            
            let node: DagNode = rmp_serde::from_slice(&plaintext)?;
            Ok(Some(node))
        } else {
            Ok(None)
        }
    }

    /// Iterate over all stored nodes and decrypt.
    pub fn load_all(&self) -> impl Iterator<Item = (Hash32, DagNode)> + '_ {
        self.db.iter().filter_map(move |res| {
            if let Ok((k, v)) = res {
                let mut hash = [0u8; 32];
                if k.len() == 32 {
                    hash.copy_from_slice(&k);
                    let nonce = Nonce::from_slice(&hash[0..12]);
                    if let Ok(plaintext) = self.cipher.decrypt(nonce, v.as_ref()) {
                        if let Ok(node) = rmp_serde::from_slice::<DagNode>(&plaintext) {
                            return Some((hash, node));
                        }
                    }
                }
            }
            None
        })
    }
}
