//! SQLite persistence. Uses `rusqlite` with the `bundled` feature so the tool
//! stays a single binary.
//!
//! Caches analysis results, explanations and Explorer responses. `Store` must be
//! cloneable and shareable across tasks (`Arc<Mutex<Connection>>` inside).

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

/// Schema applied on every `open()` / `in_memory()`.
///
/// `analysis` is keyed by `(fen, depth, multipv, engine)`. `engine` is the `id
/// name` line the binary announced (`kibitz_engine::Engine::name`): an evaluation
/// is only ever valid for the engine that produced it, and `delta` compares two
/// adjacent positions, so serving one engine's number next to another's corrupts
/// the classification outright.
///
/// `book` is keyed by `(fen, database)` so that `masters` and `lichess` rows for
/// the same position coexist instead of overwriting each other.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS analysis (
  fen      TEXT NOT NULL,
  depth    INTEGER NOT NULL,
  multipv  INTEGER NOT NULL,
  engine   TEXT NOT NULL,
  result   TEXT NOT NULL,
  PRIMARY KEY (fen, depth, multipv, engine)
);

CREATE TABLE IF NOT EXISTS explanation (
  context_hash TEXT PRIMARY KEY,
  text         TEXT NOT NULL,
  model        TEXT NOT NULL,
  lang         TEXT NOT NULL,
  created_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS book (
  fen        TEXT NOT NULL,
  database   TEXT NOT NULL,
  result     TEXT NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (fen, database)
);
"#;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    /// Open the file and create the schema (`CREATE TABLE IF NOT EXISTS`).
    pub fn open(path: impl AsRef<Path>) -> Result<Store, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        // WAL survives across connections; it is a no-op for in-memory databases.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Store::init(conn)
    }

    /// In-memory database for tests.
    pub fn in_memory() -> Result<Store, StoreError> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store, StoreError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Before `CREATE TABLE IF NOT EXISTS`, which would silently leave an old
        // table in place and let every later statement fail on the missing column.
        migrate(&conn)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// A poisoned lock only means some other task panicked while holding the
    /// connection; the connection itself is still usable, so recover from it
    /// rather than propagating the panic.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ─── analysis ───────────────────────────────────────
    // PRIMARY KEY (fen, depth, multipv, engine). `fen` is the normalized FEN from
    // `kibitz_core::normalize_fen` — move counters stripped. `engine` is the
    // engine's `id name`, so upgrading Stockfish starts a fresh namespace instead
    // of serving the old binary's numbers.

    pub fn get_analysis(
        &self,
        fen: &str,
        depth: u8,
        multipv: usize,
        engine: &str,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.lock();
        let result = conn
            .query_row(
                "SELECT result FROM analysis
                 WHERE fen = ?1 AND depth = ?2 AND multipv = ?3 AND engine = ?4",
                params![fen, depth as i64, multipv as i64, engine],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(result)
    }

    pub fn put_analysis(
        &self,
        fen: &str,
        depth: u8,
        multipv: usize,
        engine: &str,
        result_json: &str,
    ) -> Result<(), StoreError> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO analysis (fen, depth, multipv, engine, result)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(fen, depth, multipv, engine) DO UPDATE SET result = excluded.result",
            params![fen, depth as i64, multipv as i64, engine, result_json],
        )?;
        Ok(())
    }

    // ─── explanation ────────────────────────────────────
    // PRIMARY KEY (context_hash). The hash covers the AnalysisContext, the model
    // **and the language** — otherwise a Japanese explanation would be served for
    // an English request.

    pub fn get_explanation(&self, context_hash: &str) -> Result<Option<String>, StoreError> {
        let conn = self.lock();
        let text = conn
            .query_row(
                "SELECT text FROM explanation WHERE context_hash = ?1",
                params![context_hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(text)
    }

    pub fn put_explanation(
        &self,
        context_hash: &str,
        text: &str,
        model: &str,
        lang: &str,
    ) -> Result<(), StoreError> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO explanation (context_hash, text, model, lang, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(context_hash) DO UPDATE SET
               text = excluded.text,
               model = excluded.model,
               lang = excluded.lang,
               created_at = excluded.created_at",
            params![context_hash, text, model, lang, now_unix()],
        )?;
        Ok(())
    }

    // ─── book ───────────────────────────────────────────
    // PRIMARY KEY (fen, database). The Explorer response goes in as raw JSON;
    // masters and lichess results must never be mixed.

    pub fn get_book(&self, fen: &str, database: &str) -> Result<Option<String>, StoreError> {
        let conn = self.lock();
        let result = conn
            .query_row(
                "SELECT result FROM book WHERE fen = ?1 AND database = ?2",
                params![fen, database],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(result)
    }

    pub fn put_book(&self, fen: &str, database: &str, result_json: &str) -> Result<(), StoreError> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO book (fen, database, result, fetched_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(fen, database) DO UPDATE SET
               result = excluded.result,
               fetched_at = excluded.fetched_at",
            params![fen, database, result_json, now_unix()],
        )?;
        Ok(())
    }
}

/// Bring a database written by an older kibitz up to [`SCHEMA`].
///
/// Only one migration exists so far: `analysis` gained an `engine` column and a
/// four-column primary key. A `~/.kibitz/kibitz.db` from before that change still
/// holds the three-column table, and `CREATE TABLE IF NOT EXISTS` would leave it
/// alone — every read and write would then fail on `no such column: engine`.
///
/// The old rows are dropped rather than carried over. They cannot be attributed to
/// any engine, which is exactly the bug the column exists to fix; parking them under
/// a placeholder name would only keep bytes that no live engine could ever match.
/// The table is a cache of a reproducible computation, so the cost is re-analysis,
/// not lost work. `explanation` and `book` are untouched — they were never keyed on
/// the engine.
///
/// The presence of the column is the detector, rather than a stored schema version:
/// databases predating this change carry no version stamp to read, so the table's
/// own shape is the only thing that can be trusted.
fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let has_legacy_analysis: bool = conn.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'analysis'
         ) AND NOT EXISTS(
           SELECT 1 FROM pragma_table_info('analysis') WHERE name = 'engine'
         )",
        [],
        |row| row.get(0),
    )?;
    if !has_legacy_analysis {
        return Ok(());
    }

    tracing::warn!(
        "dropping the analysis cache: it predates engine-keyed entries, so its rows \
         cannot be attributed to an engine and have to be recomputed"
    );
    conn.execute_batch("DROP TABLE analysis")?;
    Ok(())
}

/// Seconds since the Unix epoch, used for the `created_at` / `fetched_at`
/// columns.
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Default database location (`~/.kibitz/kibitz.db`).
///
/// The `~/.kibitz` directory is created if it does not exist yet; a failure to
/// create it is left to surface later, when the database is actually opened.
pub fn default_path() -> std::path::PathBuf {
    let dir = default_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("failed to create {}: {e}", dir.display());
    }
    dir.join(DB_FILE_NAME)
}

const DB_FILE_NAME: &str = "kibitz.db";

/// `~/.kibitz`, without creating anything.
fn default_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join(".kibitz")
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_round_trip() {
        let store = Store::in_memory().unwrap();
        assert!(store.get_analysis("fen", 20, 3, "Stockfish 18").unwrap().is_none());

        store
            .put_analysis("fen", 20, 3, "Stockfish 18", r#"{"a":1}"#)
            .unwrap();
        assert_eq!(
            store
                .get_analysis("fen", 20, 3, "Stockfish 18")
                .unwrap()
                .as_deref(),
            Some(r#"{"a":1}"#)
        );
        // Every component of the primary key is part of the lookup.
        assert!(store.get_analysis("fen", 21, 3, "Stockfish 18").unwrap().is_none());
        assert!(store.get_analysis("fen", 20, 1, "Stockfish 18").unwrap().is_none());
        assert!(store.get_analysis("fen", 20, 3, "Stockfish 17").unwrap().is_none());
    }

    #[test]
    fn analysis_conflict_overwrites() {
        let store = Store::in_memory().unwrap();
        store.put_analysis("fen", 20, 3, "engine", "old").unwrap();
        store.put_analysis("fen", 20, 3, "engine", "new").unwrap();
        assert_eq!(
            store.get_analysis("fen", 20, 3, "engine").unwrap().as_deref(),
            Some("new")
        );
        assert_eq!(count(&store, "analysis"), 1);
    }

    /// The point of the `engine` column: upgrading Stockfish must start a fresh
    /// namespace, never inherit the previous binary's evaluations.
    #[test]
    fn analysis_engines_do_not_mix() {
        let store = Store::in_memory().unwrap();
        store
            .put_analysis("fen", 20, 3, "Stockfish 17", "seventeen")
            .unwrap();
        store
            .put_analysis("fen", 20, 3, "Stockfish 18", "eighteen")
            .unwrap();

        assert_eq!(
            store
                .get_analysis("fen", 20, 3, "Stockfish 17")
                .unwrap()
                .as_deref(),
            Some("seventeen")
        );
        assert_eq!(
            store
                .get_analysis("fen", 20, 3, "Stockfish 18")
                .unwrap()
                .as_deref(),
            Some("eighteen")
        );
        // Two rows, not one overwriting the other.
        assert_eq!(count(&store, "analysis"), 2);
        // And an engine that wrote nothing gets nothing.
        assert!(store.get_analysis("fen", 20, 3, "Ethereal 14").unwrap().is_none());
    }

    /// A `~/.kibitz/kibitz.db` written before the `engine` column existed must keep
    /// working: the stale `analysis` table is rebuilt, and the tables that were
    /// never engine-keyed keep their rows.
    #[test]
    fn opens_a_database_written_with_the_pre_engine_schema() {
        let dir = std::env::temp_dir().join(format!(
            "kibitz-store-legacy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kibitz.db");

        // Exactly the schema kibitz shipped before the engine became part of the key.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS analysis (
                  fen      TEXT NOT NULL,
                  depth    INTEGER NOT NULL,
                  multipv  INTEGER NOT NULL,
                  result   TEXT NOT NULL,
                  PRIMARY KEY (fen, depth, multipv)
                );
                CREATE TABLE IF NOT EXISTS explanation (
                  context_hash TEXT PRIMARY KEY,
                  text         TEXT NOT NULL,
                  model        TEXT NOT NULL,
                  lang         TEXT NOT NULL,
                  created_at   INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS book (
                  fen        TEXT NOT NULL,
                  database   TEXT NOT NULL,
                  result     TEXT NOT NULL,
                  fetched_at INTEGER NOT NULL,
                  PRIMARY KEY (fen, database)
                );
                INSERT INTO analysis VALUES ('fen', 20, 3, 'from-the-old-engine');
                INSERT INTO explanation VALUES ('h', 'kept', 'sonnet', 'en', 1);
                INSERT INTO book VALUES ('fen', 'masters', 'kept', 1);
                "#,
            )
            .unwrap();
        }

        // Opening must neither panic nor error.
        let store = Store::open(&path).unwrap();

        // The unattributable rows are gone rather than served to some engine.
        assert!(store.get_analysis("fen", 20, 3, "Stockfish 18").unwrap().is_none());
        assert_eq!(count(&store, "analysis"), 0);

        // Reads and writes work against the new key.
        store
            .put_analysis("fen", 20, 3, "Stockfish 18", "fresh")
            .unwrap();
        assert_eq!(
            store
                .get_analysis("fen", 20, 3, "Stockfish 18")
                .unwrap()
                .as_deref(),
            Some("fresh")
        );

        // Tables that were never engine-keyed keep everything.
        assert_eq!(store.get_explanation("h").unwrap().as_deref(), Some("kept"));
        assert_eq!(
            store.get_book("fen", "masters").unwrap().as_deref(),
            Some("kept")
        );

        // And re-opening an already-migrated database is a no-op.
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .get_analysis("fen", 20, 3, "Stockfish 18")
                .unwrap()
                .as_deref(),
            Some("fresh")
        );

        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explanation_round_trip() {
        let store = Store::in_memory().unwrap();
        assert!(store.get_explanation("h").unwrap().is_none());

        store.put_explanation("h", "hello", "sonnet", "en").unwrap();
        assert_eq!(
            store.get_explanation("h").unwrap().as_deref(),
            Some("hello")
        );

        let (model, lang, created_at) = explanation_row(&store, "h");
        assert_eq!(model, "sonnet");
        assert_eq!(lang, "en");
        assert!(created_at > 0);
    }

    #[test]
    fn explanation_conflict_overwrites() {
        let store = Store::in_memory().unwrap();
        store.put_explanation("h", "hello", "sonnet", "en").unwrap();
        store
            .put_explanation("h", "こんにちは", "opus", "ja")
            .unwrap();

        assert_eq!(
            store.get_explanation("h").unwrap().as_deref(),
            Some("こんにちは")
        );
        let (model, lang, _) = explanation_row(&store, "h");
        assert_eq!(model, "opus");
        assert_eq!(lang, "ja");
        assert_eq!(count(&store, "explanation"), 1);
    }

    #[test]
    fn book_round_trip() {
        let store = Store::in_memory().unwrap();
        assert!(store.get_book("fen", "masters").unwrap().is_none());

        store.put_book("fen", "masters", r#"{"moves":[]}"#).unwrap();
        assert_eq!(
            store.get_book("fen", "masters").unwrap().as_deref(),
            Some(r#"{"moves":[]}"#)
        );
    }

    #[test]
    fn book_conflict_overwrites() {
        let store = Store::in_memory().unwrap();
        store.put_book("fen", "masters", "old").unwrap();
        store.put_book("fen", "masters", "new").unwrap();
        assert_eq!(
            store.get_book("fen", "masters").unwrap().as_deref(),
            Some("new")
        );
        assert_eq!(count(&store, "book"), 1);
    }

    #[test]
    fn book_databases_do_not_mix() {
        let store = Store::in_memory().unwrap();
        store.put_book("fen", "masters", "masters-json").unwrap();
        store.put_book("fen", "lichess", "lichess-json").unwrap();

        assert_eq!(
            store.get_book("fen", "masters").unwrap().as_deref(),
            Some("masters-json")
        );
        assert_eq!(
            store.get_book("fen", "lichess").unwrap().as_deref(),
            Some("lichess-json")
        );
        assert_eq!(count(&store, "book"), 2);
    }

    #[test]
    fn store_is_shareable_across_threads() {
        let store = Store::in_memory().unwrap();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .put_analysis(&format!("fen{i}"), 20, 3, "engine", "result")
                        .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(count(&store, "analysis"), 8);
    }

    #[test]
    fn open_creates_file_and_parent_directory() {
        let dir = std::env::temp_dir().join(format!("kibitz-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("kibitz.db");

        let store = Store::open(&path).unwrap();
        store
            .put_analysis("fen", 20, 3, "engine", "result")
            .unwrap();
        drop(store);
        assert!(path.exists());

        // Re-opening an existing file must not wipe it.
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .get_analysis("fen", 20, 3, "engine")
                .unwrap()
                .as_deref(),
            Some("result")
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `default_path` itself is not called here: it would create `~/.kibitz` as
    /// a side effect of running the test suite.
    #[test]
    fn default_path_is_under_a_dot_kibitz_directory() {
        let path = default_dir().join(DB_FILE_NAME);
        assert!(path.ends_with(".kibitz/kibitz.db"), "{}", path.display());
        assert!(path.is_absolute() || std::env::var_os("HOME").is_none());
    }

    fn count(store: &Store, table: &str) -> i64 {
        let conn = store.lock();
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    fn explanation_row(store: &Store, hash: &str) -> (String, String, i64) {
        let conn = store.lock();
        conn.query_row(
            "SELECT model, lang, created_at FROM explanation WHERE context_hash = ?1",
            params![hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    }
}
