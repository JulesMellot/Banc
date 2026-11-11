//! Base SQLite du serveur et ses migrations.
//!
//! Les migrations sont les fichiers `migrations/NNNN_nom.sql` du dépôt,
//! embarqués à la compilation et appliqués dans l'ordre à l'ouverture.
//! `migrations/schema.sql` garde le schéma obtenu, contrôlé par
//! `cargo xtask check migrations`.

use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

static MIGRATIONS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../migrations");

/// Fichier du schéma de référence, à côté des migrations.
pub const SCHEMA_FILE: &str = "schema.sql";

const SCHEMA_HEADER: &str = "\
-- Schéma obtenu après toutes les migrations.
-- Écrit par `cargo xtask schema`, ne pas modifier à la main.
";

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("migration mal nommée : {0} (attendu : NNNN_nom_en_minuscules.sql)")]
    BadName(String),
    #[error("numérotation des migrations : {found} trouvé là où {expected:04} est attendu")]
    BadNumber { expected: u32, found: String },
    #[error("migration {0} : le fichier n'est pas en UTF-8")]
    NotUtf8(String),
    #[error("impossible de créer le dossier de la base {dir}")]
    CreateDir {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("erreur SQLite")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration refusée")]
    Migration(#[from] rusqlite_migration::Error),
}

/// Trie et contrôle une liste de fichiers `(nom, contenu)` : les noms suivent
/// `NNNN_nom.sql` et les numéros se suivent à partir de 0001, sans trou ni
/// doublon. Renvoie les contenus dans l'ordre d'application.
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

/// Numéro d'un fichier `NNNN_nom.sql`, ou `None` si le nom ne suit pas la règle.
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

/// Les migrations embarquées, dans l'ordre. `None` tant que le dépôt n'en
/// contient aucune : `rusqlite_migration` refuse une liste vide.
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

/// Ouvre la base, en la créant au besoin, et applique les migrations manquantes.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|source| DbError::CreateDir {
            dir: dir.to_path_buf(),
            source,
        })?;
    }
    prepare(Connection::open(path)?)
}

/// Base en mémoire avec toutes les migrations, pour les tests.
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

/// Le schéma de la base, sous la forme gardée dans `migrations/schema.sql`.
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
    fn noms_de_migration() {
        assert_eq!(migration_number("0001_socle.sql"), Some(1));
        assert_eq!(migration_number("0042_blob_locations_v2.sql"), Some(42));
        for name in [
            "1_socle.sql",
            "00001_socle.sql",
            "0001-socle.sql",
            "0001_.sql",
            "0001_Socle.sql",
            "0001_socle.txt",
            "socle.sql",
            "schema.sql",
        ] {
            assert_eq!(migration_number(name), None, "{name}");
        }
    }

    #[test]
    fn ordre_d_application() {
        let files = vec![
            ("0002_machines.sql", "B"),
            ("0001_socle.sql", "A"),
            ("0003_invites.sql", "C"),
        ];
        assert_eq!(ordered(files).unwrap(), ["A", "B", "C"]);
        assert!(ordered(Vec::new()).unwrap().is_empty());
    }

    #[test]
    fn trou_dans_la_numerotation() {
        let err = ordered(vec![("0001_socle.sql", ""), ("0003_invites.sql", "")]).unwrap_err();
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
    fn numero_en_double() {
        let err = ordered(vec![("0001_socle.sql", ""), ("0001_autre.sql", "")]).unwrap_err();
        assert!(
            matches!(err, DbError::BadNumber { expected: 2, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn nom_refuse() {
        let err = ordered(vec![("socle.sql", "")]).unwrap_err();
        assert!(matches!(err, DbError::BadName(_)), "{err:?}");
    }

    #[test]
    fn ouverture_cree_le_dossier_et_reste_rejouable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("donnees/banc.sqlite");

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
    fn schema_trie_et_termine_par_des_points_virgules() {
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
