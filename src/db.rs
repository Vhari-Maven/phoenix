//! SQLite database for caching version information.
//!
//! This module provides a persistent cache that maps executable SHA256 hashes
//! to version information. This avoids re-parsing VERSION.txt for previously
//! seen game builds.
//!
//! The database is stored at `%APPDATA%\phoenix\Phoenix\data\phoenix.db`.
//!
//! Stable release hashes are loaded via `app_data::stable_versions()`,
//! enabling instant version identification.

use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;

use crate::app_data::stable_versions;

/// Version information stored in the database
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Version string (e.g., "0.F-3" or "2024-01-15-1234")
    pub version: String,
    /// Whether this is a stable release
    pub stable: bool,
    /// Release date (ISO format)
    pub released_on: Option<String>,
}

/// Database manager for caching version information
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Get the database file path
    pub fn db_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("com", "phoenix", "Phoenix")
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;

        let data_dir = dirs.data_dir();
        std::fs::create_dir_all(data_dir)?;

        Ok(data_dir.join("phoenix.db"))
    }

    /// Open or create the database
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;
        let conn = Connection::open(&path)?;

        let db = Self { conn };
        db.init_schema()?;

        tracing::info!("Opened database at {:?}", path);
        Ok(db)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS game_versions (
                sha256 TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                stable INTEGER NOT NULL DEFAULT 0,
                build_number INTEGER,
                released_on TEXT,
                discovered_on TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_version ON game_versions(version);

            -- Cache for executable SHA256 hashes (avoids recalculating on every startup)
            CREATE TABLE IF NOT EXISTS exe_hash_cache (
                path TEXT PRIMARY KEY,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                sha256 TEXT NOT NULL
            );
            "
        )?;
        Ok(())
    }

    /// Look up version by SHA256 hash
    /// First checks stable versions from stable_hashes.toml, then the database
    pub fn get_version(&self, sha256: &str) -> Result<Option<VersionInfo>> {
        // Check stable versions first (instant lookup)
        if let Some(version) = stable_versions().get(sha256) {
            return Ok(Some(VersionInfo {
                version: version.clone(),
                stable: true,
                released_on: None,
            }));
        }

        // Check database for cached experimental versions
        let mut stmt = self.conn.prepare(
            "SELECT version, stable, build_number, released_on
             FROM game_versions WHERE sha256 = ?"
        )?;

        let result = stmt.query_row(params![sha256], |row| {
            Ok(VersionInfo {
                version: row.get(0)?,
                stable: row.get::<_, i32>(1)? != 0,
                released_on: row.get(3)?,
            })
        });

        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get cached SHA256 hash for an executable if file metadata matches
    pub fn get_cached_hash(&self, path: &str, size: u64, mtime: i64) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT sha256 FROM exe_hash_cache WHERE path = ? AND size = ? AND mtime = ?"
        )?;

        let result = stmt.query_row(params![path, size as i64, mtime], |row| {
            row.get::<_, String>(0)
        });

        match result {
            Ok(hash) => Ok(Some(hash)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Store SHA256 hash in cache with file metadata
    pub fn store_cached_hash(&self, path: &str, size: u64, mtime: i64, sha256: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO exe_hash_cache (path, size, mtime, sha256) VALUES (?, ?, ?, ?)",
            params![path, size as i64, mtime, sha256],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stable_version_lookup() {
        // Create in-memory database for testing
        let conn = Connection::open_in_memory().unwrap();
        let db = Database { conn };
        db.init_schema().unwrap();

        // Test known stable version
        let result = db.get_version(
            "3e0b15543015389c34ad679a931186a1264dbccb010b813f63b6caef2d158dc8"
        ).unwrap();

        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.version, "0.F-3");
        assert!(info.stable);
    }

    #[test]
    fn test_unknown_version() {
        let conn = Connection::open_in_memory().unwrap();
        let db = Database { conn };
        db.init_schema().unwrap();

        let result = db.get_version("unknown_hash_that_does_not_exist").unwrap();
        assert!(result.is_none());
    }
}
