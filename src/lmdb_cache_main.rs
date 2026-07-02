// SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
// Copyright (C) 2026  Red Hat, Inc.

use crate::lmdb_cache::{ApiCache, LmdbCacheImpl};
use anyhow::{Context, Result};
use lmdb::{Cursor, Transaction};
use std::path::Path;

pub fn create_from_path(path: &str, overwrite: bool) -> Result<LmdbCacheImpl> {
    LmdbCacheImpl::create_from_path(path, overwrite)
}

impl LmdbCacheImpl {
    fn create_env_readonly(path: &str) -> Result<lmdb::Environment> {
        Self::create_env_internal(path, true)
    }

    pub fn import_from_path(&self, import_path: &str) -> Result<()> {
        let expanded = shellexpand::full(import_path)?;
        let import_dir = Path::new(expanded.as_ref());
        if !import_dir.exists() {
            return Err(anyhow::anyhow!(
                "Import path does not exist: {}",
                import_path
            ));
        }

        let import_env =
            Self::create_env_readonly(expanded.as_ref()).context("open import lmdb env")?;

        for &db_name in Self::DB_NAMES {
            let import_db = match import_env.open_db(Some(db_name)) {
                Ok(db) => db,
                Err(e) => {
                    log::warn!("Database {} not found in import cache: {}", db_name, e);
                    continue;
                }
            };

            let current_db = self
                .lmdb_env
                .create_db(Some(db_name), lmdb::DatabaseFlags::empty())
                .context(format!("Failed to open/create current db: {}", db_name))?;

            let mut current_txn = self
                .lmdb_env
                .begin_rw_txn()
                .context("begin rw txn on current env")?;

            let import_txn = import_env
                .begin_ro_txn()
                .context("begin ro txn on import env")?;

            {
                let mut cursor = import_txn
                    .open_ro_cursor(import_db)
                    .context("open ro cursor")?;
                for (key, value) in cursor.iter() {
                    current_txn
                        .put(current_db, &key, &value, lmdb::WriteFlags::empty())
                        .context("write kv to current db")?;
                }
            }

            current_txn.commit().context("commit current txn")?;
        }

        Ok(())
    }
}

// Local Variables:
// rust-format-on-save: t
// End:
