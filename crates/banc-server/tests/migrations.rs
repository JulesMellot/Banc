//! Migration check: they apply to an empty database and give exactly the
//! schema kept in `migrations/schema.sql`.
//!
//! Run by `cargo xtask check migrations`. `cargo xtask schema` rewrites the
//! reference file after a migration is added.

use std::path::PathBuf;

use banc_server::db;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../migrations")
        .join(db::SCHEMA_FILE)
}

#[test]
fn les_migrations_sont_valides() {
    if let Some(migrations) = db::migrations().unwrap() {
        migrations.validate().unwrap();
    }
}

#[test]
fn le_schema_correspond_au_fichier_de_reference() {
    let conn = db::open_in_memory().unwrap();
    let actual = db::schema(&conn).unwrap();
    let path = schema_path();

    if std::env::var_os("BANC_WRITE_SCHEMA").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "migrations/schema.sql ne correspond plus aux migrations.\n\
         Lance `cargo xtask schema` et relis le diff.\n\n\
         --- obtenu ---\n{actual}\n--- attendu ---\n{expected}"
    );
}
