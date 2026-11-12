# Server database migrations

A migration is a file named `NNNN_lowercase_name.sql`. Numbers follow each other from `0001`, with no gap. A migration merged into `main` is never modified again: add a new one instead.

`banc-server` embeds these files at compile time and applies the missing ones when it opens the database.

`schema.sql` keeps the schema obtained once all migrations are applied to an empty database. After adding a migration:

```bash
cargo xtask schema
```

Review the diff of `schema.sql` before committing. `cargo xtask check migrations` fails if the file no longer matches the migrations.

The target data model is described in section 3 of `docs/PLAN.md`. The first tables arrive in phase 1.
