use std::path::Path;

use rusqlite::Connection;

pub const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

const SCHEMA: &str = "
CREATE TABLE categories (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK (name <> ''),
    position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE folders (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK (name <> ''),
    category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    status_id TEXT NOT NULL UNIQUE,
    source_url TEXT NOT NULL,
    author_screen_name TEXT NOT NULL DEFAULT '',
    author_name TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL DEFAULT '',
    lang TEXT,
    posted_at TEXT,
    saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    state TEXT NOT NULL CHECK (state IN ('pending', 'done', 'failed')),
    failure TEXT,
    snapshot TEXT,
    folder_id INTEGER REFERENCES folders(id) ON DELETE CASCADE
);
CREATE INDEX posts_saved_at ON posts (saved_at, id);
CREATE INDEX posts_folder ON posts (folder_id);
CREATE TABLE media (
    post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    position INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('photo', 'video', 'gif')),
    file TEXT NOT NULL,
    thumbnail TEXT,
    width INTEGER,
    height INTEGER,
    remote_url TEXT NOT NULL,
    PRIMARY KEY (post_id, role, position)
);
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    key TEXT NOT NULL UNIQUE,
    color TEXT
);
CREATE TABLE post_tags (
    post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (post_id, tag_id)
);
CREATE INDEX post_tags_tag ON post_tags (tag_id);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE credentials (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash TEXT NOT NULL
);
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    expires_at TEXT NOT NULL
);
";

const MIGRATIONS: &[&str] = &[
    "ALTER TABLE media ADD COLUMN duration REAL;",
    // A YouTube video id (11 characters) never looks like an X status id, so status_id stays unique on its own.
    "ALTER TABLE posts ADD COLUMN source TEXT NOT NULL DEFAULT 'x' CHECK (source IN ('x', 'youtube'));",
    // SQLite cannot change a CHECK constraint, so the table is rebuilt to allow web pages.
    "CREATE TABLE posts_new (
        id INTEGER PRIMARY KEY,
        status_id TEXT NOT NULL UNIQUE,
        source_url TEXT NOT NULL,
        author_screen_name TEXT NOT NULL DEFAULT '',
        author_name TEXT NOT NULL DEFAULT '',
        text TEXT NOT NULL DEFAULT '',
        lang TEXT,
        posted_at TEXT,
        saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        state TEXT NOT NULL CHECK (state IN ('pending', 'done', 'failed')),
        failure TEXT,
        snapshot TEXT,
        folder_id INTEGER REFERENCES folders(id) ON DELETE CASCADE,
        source TEXT NOT NULL DEFAULT 'x' CHECK (source IN ('x', 'youtube', 'web'))
    );
    INSERT INTO posts_new SELECT * FROM posts;
    DROP TABLE posts;
    ALTER TABLE posts_new RENAME TO posts;
    CREATE INDEX posts_saved_at ON posts (saved_at, id);
    CREATE INDEX posts_folder ON posts (folder_id);",
];

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    let mut version = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if version == 0 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA} PRAGMA user_version = 1; COMMIT;"))?;
        version = 1;
    }
    // Rebuilding a table must not cascade deletes into the tables that point at it.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    for (i, migration) in MIGRATIONS.iter().enumerate().skip(version - 1) {
        conn.execute_batch(&format!(
            "BEGIN; {migration} PRAGMA user_version = {}; COMMIT;",
            i + 2
        ))?;
    }
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_a_version_1_database_without_losing_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arcman.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!("{SCHEMA} PRAGMA user_version = 1;"))
                .unwrap();
            conn.execute("INSERT INTO categories (name) VALUES ('kept')", [])
                .unwrap();
            conn.execute_batch(
                "INSERT INTO posts (id, status_id, source_url, state) VALUES (7, '1', 'u', 'done');
                 INSERT INTO media (post_id, role, position, kind, file, remote_url) VALUES (7, 'main', 0, 'photo', 'f', 'r');
                 INSERT INTO tags (id, name, key) VALUES (1, 't', 't');
                 INSERT INTO post_tags VALUES (7, 1);",
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len() + 1);
        conn.execute("UPDATE media SET duration = 1.5", []).unwrap();
        conn.execute("UPDATE posts SET source = 'web'", []).unwrap();
        let kept: (i64, i64, String) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM media), (SELECT COUNT(*) FROM post_tags), source FROM posts WHERE id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(kept, (1, 1, "web".into()));
        // Foreign keys are back on: deleting the post still cascades.
        conn.execute("DELETE FROM posts WHERE id = 7", []).unwrap();
        let media: i64 = conn
            .query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0))
            .unwrap();
        assert_eq!(media, 0);
        let name: String = conn
            .query_row("SELECT name FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "kept");
    }

    #[test]
    fn reopening_an_up_to_date_database_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arcman.db");
        drop(open(&path).unwrap());
        assert!(open(&path).is_ok());
    }
}
