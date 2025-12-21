use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Known stable version SHA256 hashes (from original launcher)
/// These are hardcoded for instant lookup of stable releases
fn stable_versions() -> &'static HashMap<&'static str, &'static str> {
    static STABLE_SHA256: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    STABLE_SHA256.get_or_init(|| {
        let mut m = HashMap::new();
        // 0.C Cooper
        m.insert("2d7bbf426572e2b21aede324c8d89c9ad84529a05a4ac99a914f22b2b1e1405e", "0.C");
        // 0.D Danny
        m.insert("0454ed2bbc4a6c1c8cca5c360533513eb2a1d975816816d7c13ff60e276d431b", "0.D");
        m.insert("7f914145248cebfd4d1a6d4b1ff932a478504b1e7e4c689aab97b8700e079f61", "0.D");
        // 0.E Ellison
        m.insert("bdd4f539767fd970beeab271e0e3774ba3022faeff88c6186b389e6bbe84bc75", "0.E");
        m.insert("8adea7b3bc81fa9e4594b19553faeb591846295f47b67110dbd16eed8b37e62b", "0.E");
        // 0.E-1
        m.insert("fb7db2b3cf101e19565ce515c012a089a75f54da541cd458144dc8483b5e59c8", "0.E-1");
        m.insert("1068867549c1a24ae241a886907651508830ccd9c091bad27bacbefabab99acc", "0.E-1");
        // 0.E-2
        m.insert("0ce61cdfc299661382e30da133f7356b4faea38865ec0947139a08f40b595728", "0.E-2");
        m.insert("c9ca51bd1e7549b0820fe736c10b3e73d358700c3460a0227fade59e9754e03d", "0.E-2");
        // 0.E-3
        m.insert("563bd13cff18c4271c43c18568237046d1fd18ae200f7e5cdd969b80e6992967", "0.E-3");
        m.insert("e4874bbb8e0a7b1e52b4dedb99575e2a90bfe84e74c36db58510f9973400077d", "0.E-3");
        // 0.F Frank
        m.insert("1f5beb8b3dcb5ca1f704b816864771e2dd8ff38ca435a4abdb9a59e4bb95d099", "0.F");
        m.insert("2794df225787174c6f5d8557d63f434a46a82f562c0395294901fb5d5d10d564", "0.F");
        // 0.F-1
        m.insert("960140f7926267b56ef6933670b7a73d00087bd53149e9e63c48a8631cfbed53", "0.F-1");
        m.insert("c87f226d8b4e6543fbc8527d645cf4342b5e1036e94e16920381d7e5b5b9e34f", "0.F-1");
        // 0.F-2
        m.insert("5da7ebd7ab07ebf755e445440210309eda0ae8f5924026d401b9eb5c52c5b6e7", "0.F-2");
        m.insert("6870353e6d142735dfd21dec1eaf6b39af088daf5eef27b02e53ebb1c9eca684", "0.F-2");
        // 0.F-3
        m.insert("3e0b15543015389c34ad679a931186a1264dbccb010b813f63b6caef2d158dc8", "0.F-3");
        m.insert("59404eeb88539b20c9ffbbcbe86a7e5c20267375975306245862c7fb731a5973", "0.F-3");
        m
    })
}

/// Version information stored in the database
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Version string (e.g., "0.F-3" or "2024-01-15-1234")
    pub version: String,
    /// Whether this is a stable release
    pub stable: bool,
    /// Build number (for experimental builds)
    pub build_number: Option<i64>,
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
            "
        )?;
        Ok(())
    }

    /// Look up version by SHA256 hash
    /// First checks hardcoded stable versions, then the database
    pub fn get_version(&self, sha256: &str) -> Result<Option<VersionInfo>> {
        // Check hardcoded stable versions first (instant lookup)
        if let Some(&version) = stable_versions().get(sha256) {
            return Ok(Some(VersionInfo {
                version: version.to_string(),
                stable: true,
                build_number: None,
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
                build_number: row.get(2)?,
                released_on: row.get(3)?,
            })
        });

        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Store a new version mapping
    pub fn store_version(&self, sha256: &str, info: &VersionInfo) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO game_versions
             (sha256, version, stable, build_number, released_on)
             VALUES (?, ?, ?, ?, ?)",
            params![
                sha256,
                &info.version,
                if info.stable { 1 } else { 0 },
                info.build_number,
                &info.released_on,
            ],
        )?;

        tracing::debug!("Stored version {} for hash {}", info.version, &sha256[..8]);
        Ok(())
    }

    /// Get count of cached versions
    pub fn version_count(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM game_versions",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
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
    fn test_store_and_retrieve_version() {
        let conn = Connection::open_in_memory().unwrap();
        let db = Database { conn };
        db.init_schema().unwrap();

        let sha256 = "abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234";
        let info = VersionInfo {
            version: "2024-01-15-1234".to_string(),
            stable: false,
            build_number: Some(1234),
            released_on: Some("2024-01-15".to_string()),
        };

        db.store_version(sha256, &info).unwrap();

        let retrieved = db.get_version(sha256).unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.version, "2024-01-15-1234");
        assert!(!retrieved.stable);
        assert_eq!(retrieved.build_number, Some(1234));
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
