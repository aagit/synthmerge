// SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
// Copyright (C) 2026  Red Hat, Inc.

use anyhow::Result;
use lmdb::Transaction;
use sha2::{Digest, Sha256};

pub struct ApiCache {
    lmdb_env: lmdb::Environment,
    lmdb_db: lmdb::Database,
    overwrite: bool,
}

impl ApiCache {
    pub fn new(lmdb_env: lmdb::Environment, lmdb_db: lmdb::Database, overwrite: bool) -> Self {
        ApiCache {
            lmdb_env,
            lmdb_db,
            overwrite,
        }
    }

    pub fn get_cache_key(&self, url: &str, payload_json: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url);
        hasher.update(payload_json);
        let hash = hasher.finalize();
        format!("{:x}", hash)
    }

    pub fn cache_response(&self, cache_key: &String, response: String) -> Result<()> {
        let compressed =
            zstd::encode_all(response.as_bytes(), 0).expect("Failed to compress response");
        let env = &self.lmdb_env;
        let mut txn = env
            .begin_rw_txn()
            .expect("Failed to start read transaction");
        txn.put(
            self.lmdb_db,
            cache_key,
            &compressed,
            lmdb::WriteFlags::empty(),
        )
        .expect("Failed to cache response");
        txn.commit().expect("Failed to commit transaction");
        Ok(())
    }

    pub fn get_cached_response(&self, cache_key: &String) -> Option<String> {
        if self.overwrite {
            return None;
        }
        let env = &self.lmdb_env;
        let txn = env
            .begin_ro_txn()
            .expect("Failed to start read transaction");
        txn.get(self.lmdb_db, cache_key)
            .ok()
            .and_then(|v| zstd::decode_all(v).ok())
            .map(|compressed| String::from_utf8_lossy(&compressed).to_string())
    }
}

// Local Variables:
// rust-format-on-save: t
// End:
