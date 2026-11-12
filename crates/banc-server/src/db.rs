//! The server's SQLite database and its migrations.
//!
//! The migrations are the repository's `migrations/NNNN_name.sql` files,
//! embedded at compile time and applied in order on opening.
//! `migrations/schema.sql` keeps the resulting schema, checked by
//! `cargo xtask check migrations`.

use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

static MIGRATIONS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../migrations");

/// Reference schema file, next to the migrations.
pub const SCHEMA_FILE: &str = "schema.sql";

const SCHEMA_HEADER: &str = "\
-- Schema obtained after all migrations.
-- Written by `cargo xtask schema`, do not edit by hand.
";

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("badly named migration: {0} (expected: NNNN_lowercase_name.sql)")]
    BadName(String),
    #[error("migration numbering: found {found} where {expected:04} is expected")]
    BadNumber { expected: u32, found: String },
    #[error("migration {0}: the file is not UTF-8")]
    NotUtf8(String),
    #[error("cannot create the database folder {dir}")]
    CreateDir {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SQLite error")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration refused")]
    Migration(#[from] rusqlite_migration::Error),
}

/// Sorts and checks a list of `(name, content)` files: names follow
/// `NNNN_name.sql` and numbers follow each other from 0001, with no gap or
/// duplicate. Returns the contents in application order.
pub fn ordered<'a>(mut files: Vec<(&'a str, &'a str)>) -> Result<Vec<&'a str>, DbError> {
    files.sort_by_key(|(name, _)| *name);
    let mut contents = Vec::with_capacity(files.len());
    for (expected, (name, sql)) in (1..).zip(files) {
        let number = migration_number(name).ok_or_else(|| DbError::BadName(name.to_owned()))?;
        if number != expected {
            return Err(DbError::BadNumber {
                expected,
                found: name.to_owned(),
            });
        }
        contents.push(sql);
    }
    Ok(contents)
}

/// Number of a `NNNN_name.sql` file, or `None` if the name breaks the rule.
fn migration_number(file_name: &str) -> Option<u32> {
    let stem = file_name.strip_suffix(".sql")?;
    let (digits, name) = stem.split_once('_')?;
    let name_ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) || !name_ok {
        return None;
    }
    digits.parse().ok()
}

/// The embedded migrations, in order. `None` while the repository has none:
/// `rusqlite_migration` refuses an empty list.
pub fn migrations() -> Result<Option<Migrations<'static>>, DbError> {
    let mut files = Vec::new();
    for file in MIGRATIONS_DIR.files() {
        let name = file
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !name.ends_with(".sql") || name == SCHEMA_FILE {
            continue;
        }
        let sql = file
            .contents_utf8()
            .ok_or_else(|| DbError::NotUtf8(name.to_owned()))?;
        files.push((name, sql));
    }
    let steps: Vec<M<'static>> = ordered(files)?.into_iter().map(M::up).collect();
    Ok((!steps.is_empty()).then(|| Migrations::new(steps)))
}

/// Opens the database, creating it if needed, and applies missing migrations.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|source| DbError::CreateDir {
            dir: dir.to_path_buf(),
            source,
        })?;
    }
    prepare(Connection::open(path)?)
}

/// In-memory database with all migrations, for tests.
pub fn open_in_memory() -> Result<Connection, DbError> {
    prepare(Connection::open_in_memory()?)
}

fn prepare(mut conn: Connection) -> Result<Connection, DbError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if let Some(migrations) = migrations()? {
        migrations.to_latest(&mut conn)?;
    }
    Ok(conn)
}

/// The database schema, in the form kept in `migrations/schema.sql`.
pub fn schema(conn: &Connection) -> Result<String, DbError> {
    let mut statement = conn.prepare(
        "SELECT sql FROM sqlite_schema \
         WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' \
         ORDER BY tbl_name, type DESC, name",
    )?;
    let mut text = String::from(SCHEMA_HEADER);
    for sql in statement.query_map([], |row| row.get::<_, String>(0))? {
        text.push('\n');
        text.push_str(sql?.trim());
        text.push_str(";\n");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_names() {
        assert_eq!(migration_number("0001_base.sql"), Some(1));
        assert_eq!(migration_number("0042_blob_locations_v2.sql"), Some(42));
        for name in [
            "1_base.sql",
            "00001_base.sql",
            "0001-base.sql",
            "0001_.sql",
            "0001_Base.sql",
            "0001_base.txt",
            "base.sql",
            "schema.sql",
        ] {
            assert_eq!(migration_number(name), None, "{name}");
        }
    }

    #[test]
    fn application_order() {
        let files = vec![
            ("0002_machines.sql", "B"),
            ("0001_base.sql", "A"),
            ("0003_invites.sql", "C"),
        ];
        assert_eq!(ordered(files).unwrap(), ["A", "B", "C"]);
        assert!(ordered(Vec::new()).unwrap().is_empty());
    }

    #[test]
    fn gap_in_numbering() {
        let err = ordered(vec![("0001_base.sql", ""), ("0003_invites.sql", "")]).unwrap_err();
        assert!(
            matches!(&err, DbError::BadNumber { expected: 2, found } if found == "0003_invites.sql"),
            "{err:?}"
        );
        let err = ordered(vec![("0002_machines.sql", "")]).unwrap_err();
        assert!(
            matches!(err, DbError::BadNumber { expected: 1, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn duplicate_number() {
        let err = ordered(vec![("0001_base.sql", ""), ("0001_other.sql", "")]).unwrap_err();
        assert!(
            matches!(err, DbError::BadNumber { expected: 2, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn name_refused() {
        let err = ordered(vec![("base.sql", "")]).unwrap_err();
        assert!(matches!(err, DbError::BadName(_)), "{err:?}");
    }

    #[test]
    fn open_creates_the_folder_and_is_repeatable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data/banc.sqlite");

        let conn = open(&path).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let foreign_keys: bool = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert!(foreign_keys);
        drop(conn);

        open(&path).unwrap();
    }

    #[test]
    fn schema_sorted_with_semicolons() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE b (id INTEGER PRIMARY KEY, a_id INTEGER);
             CREATE TABLE a (id INTEGER PRIMARY KEY);
             CREATE INDEX b_a ON b (a_id);",
        )
        .unwrap();
        let expected = format!(
            "{SCHEMA_HEADER}\n\
             CREATE TABLE a (id INTEGER PRIMARY KEY);\n\n\
             CREATE TABLE b (id INTEGER PRIMARY KEY, a_id INTEGER);\n\n\
             CREATE INDEX b_a ON b (a_id);\n"
        );
        assert_eq!(schema(&conn).unwrap(), expected);
    }
}
