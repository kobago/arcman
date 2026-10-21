use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::{
    AppState,
    error::{ApiError, ApiResult},
};

pub fn normalize(input: &str) -> Option<(String, String)> {
    let name: String = input.nfkc().collect();
    let name = name.trim().trim_start_matches('#').trim().to_string();
    (!name.is_empty()).then(|| {
        let key = name.to_lowercase();
        (name, key)
    })
}

pub fn find_or_create(conn: &Connection, input: &str) -> ApiResult<i64> {
    let (name, key) = normalize(input).ok_or(ApiError::BadRequest("invalid_tag"))?;
    if let Some(id) = conn
        .query_row("SELECT id FROM tags WHERE key = ?1", [&key], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO tags (name, key) VALUES (?1, ?2)",
        [&name, &key],
    )?;
    Ok(conn.last_insert_rowid())
}

#[derive(Serialize)]
pub struct TagView {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub count: i64,
}

#[derive(Deserialize)]
pub struct ListQuery {
    sort: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<TagView>>> {
    let order = match q.sort.as_deref() {
        Some("count") => "count DESC, t.key",
        _ => "t.key",
    };
    let db = state.db();
    let mut stmt = db.prepare(&format!(
        "SELECT t.id, t.name, t.color, COUNT(pt.post_id) AS count
         FROM tags t LEFT JOIN post_tags pt ON pt.tag_id = t.id
         GROUP BY t.id ORDER BY {order}"
    ))?;
    let tags = stmt
        .query_map([], |r| {
            Ok(TagView {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                count: r.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(tags))
}

#[derive(Deserialize)]
pub struct TagUpdate {
    name: Option<String>,
    #[serde(default, with = "double_option")]
    color: Option<Option<String>>,
}

// A plain Option<String> cannot tell an omitted color (keep) from `"color": null` (clear).
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<String>>, D::Error> {
        Option::<String>::deserialize(d).map(Some)
    }
}

fn is_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<TagUpdate>,
) -> ApiResult<StatusCode> {
    let db = state.db();
    db.query_row("SELECT 1 FROM tags WHERE id = ?1", [id], |_| Ok(()))?;
    if let Some(name) = input.name {
        let (name, key) = normalize(&name).ok_or(ApiError::BadRequest("invalid_tag"))?;
        match db.execute(
            "UPDATE tags SET name = ?1, key = ?2 WHERE id = ?3",
            params![name, key, id],
        ) {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == ErrorCode::ConstraintViolation =>
            {
                return Err(ApiError::Conflict("tag_exists"));
            }
            r => r?,
        };
    }
    if let Some(color) = input.color {
        if color.as_deref().is_some_and(|c| !is_color(c)) {
            return Err(ApiError::BadRequest("invalid_color"));
        }
        db.execute(
            "UPDATE tags SET color = ?1 WHERE id = ?2",
            params![color, id],
        )?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    match state.db().execute("DELETE FROM tags WHERE id = ?1", [id])? {
        0 => Err(ApiError::NotFound),
        _ => Ok(StatusCode::NO_CONTENT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treats_case_whitespace_hash_and_full_width_variants_as_the_same_tag() {
        let keys: Vec<String> = ["Rust", " rust ", "#Rust", "Ｒｕｓｔ", "＃ＲＵＳＴ"]
            .iter()
            .map(|s| normalize(s).unwrap().1)
            .collect();
        assert!(keys.iter().all(|k| k == "rust"), "{keys:?}");
    }

    #[test]
    fn keeps_the_original_casing_as_the_display_name() {
        assert_eq!(normalize(" #Rust ").unwrap().0, "Rust");
    }

    #[test]
    fn rejects_names_that_are_empty_after_normalization() {
        assert_eq!(normalize("  # "), None);
    }
}
