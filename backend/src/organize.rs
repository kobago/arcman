use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AppState,
    error::{ApiError, ApiResult},
};

#[derive(Serialize)]
pub struct FolderView {
    id: i64,
    name: String,
    category_id: Option<i64>,
}

#[derive(Serialize)]
pub struct CategoryView {
    id: i64,
    name: String,
    folders: Vec<FolderView>,
}

#[derive(Serialize)]
pub struct Sidebar {
    categories: Vec<CategoryView>,
    uncategorized: Vec<FolderView>,
}

#[derive(Deserialize)]
pub struct NameInput {
    name: String,
}

#[derive(Deserialize)]
pub struct NewFolder {
    name: String,
    category_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct CategoryOrder {
    ids: Vec<i64>,
}

#[derive(Deserialize)]
pub struct FolderOrder {
    category_id: Option<i64>,
    ids: Vec<i64>,
}

fn valid_name(name: &str) -> ApiResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("invalid_name"));
    }
    Ok(name)
}

fn ensure_category(conn: &Connection, id: Option<i64>) -> ApiResult<()> {
    if let Some(id) = id {
        conn.query_row("SELECT 1 FROM categories WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .ok_or(ApiError::BadRequest("unknown_category"))?;
    }
    Ok(())
}

pub async fn sidebar(State(state): State<AppState>) -> ApiResult<Json<Sidebar>> {
    let db = state.db();
    let mut folders: Vec<FolderView> = db
        .prepare("SELECT id, name, category_id FROM folders ORDER BY position, id")?
        .query_map([], |r| {
            Ok(FolderView {
                id: r.get(0)?,
                name: r.get(1)?,
                category_id: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut categories: Vec<CategoryView> = db
        .prepare("SELECT id, name FROM categories ORDER BY position, id")?
        .query_map([], |r| {
            Ok(CategoryView {
                id: r.get(0)?,
                name: r.get(1)?,
                folders: vec![],
            })
        })?
        .collect::<Result<_, _>>()?;
    for c in &mut categories {
        let (mine, rest) = folders
            .into_iter()
            .partition(|f| f.category_id == Some(c.id));
        c.folders = mine;
        folders = rest;
    }
    Ok(Json(Sidebar {
        categories,
        uncategorized: folders,
    }))
}

pub async fn create_category(
    State(state): State<AppState>,
    Json(input): Json<NameInput>,
) -> ApiResult<Json<Value>> {
    let db = state.db();
    db.execute(
        "INSERT INTO categories (name, position)
         VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM categories))",
        [valid_name(&input.name)?],
    )?;
    Ok(Json(json!({ "id": db.last_insert_rowid() })))
}

pub async fn rename_category(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<NameInput>,
) -> ApiResult<StatusCode> {
    let n = state.db().execute(
        "UPDATE categories SET name = ?1 WHERE id = ?2",
        params![valid_name(&input.name)?, id],
    )?;
    if n == 0 {
        Err(ApiError::NotFound)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

pub async fn delete_category(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    match state
        .db()
        .execute("DELETE FROM categories WHERE id = ?1", [id])?
    {
        0 => Err(ApiError::NotFound),
        _ => Ok(StatusCode::NO_CONTENT),
    }
}

pub async fn order_categories(
    State(state): State<AppState>,
    Json(input): Json<CategoryOrder>,
) -> ApiResult<StatusCode> {
    let mut db = state.db();
    let tx = db.transaction()?;
    for (i, id) in input.ids.iter().enumerate() {
        tx.execute(
            "UPDATE categories SET position = ?1 WHERE id = ?2",
            params![i as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_folder(
    State(state): State<AppState>,
    Json(input): Json<NewFolder>,
) -> ApiResult<Json<Value>> {
    let db = state.db();
    ensure_category(&db, input.category_id)?;
    db.execute(
        "INSERT INTO folders (name, category_id, position)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(position), -1) + 1 FROM folders WHERE category_id IS ?2))",
        params![valid_name(&input.name)?, input.category_id],
    )?;
    Ok(Json(json!({ "id": db.last_insert_rowid() })))
}

pub async fn folder(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let (name, category_id, post_count): (String, Option<i64>, i64) = state.db().query_row(
        "SELECT name, category_id, (SELECT COUNT(*) FROM posts WHERE folder_id = f.id)
         FROM folders f WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(Json(
        json!({ "id": id, "name": name, "category_id": category_id, "post_count": post_count }),
    ))
}

pub async fn rename_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<NameInput>,
) -> ApiResult<StatusCode> {
    let n = state.db().execute(
        "UPDATE folders SET name = ?1 WHERE id = ?2",
        params![valid_name(&input.name)?, id],
    )?;
    if n == 0 {
        Err(ApiError::NotFound)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

pub async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let post_ids: Vec<i64> = {
        let db = state.db();
        let ids = db
            .prepare("SELECT id FROM posts WHERE folder_id = ?1")?
            .query_map([id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if db.execute("DELETE FROM folders WHERE id = ?1", [id])? == 0 {
            return Err(ApiError::NotFound);
        }
        ids
    };
    for post_id in post_ids {
        crate::posts::remove_media_dir(&state, post_id);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn order_folders(
    State(state): State<AppState>,
    Json(input): Json<FolderOrder>,
) -> ApiResult<StatusCode> {
    let mut db = state.db();
    ensure_category(&db, input.category_id)?;
    let tx = db.transaction()?;
    for (i, id) in input.ids.iter().enumerate() {
        tx.execute(
            "UPDATE folders SET category_id = ?1, position = ?2 WHERE id = ?3",
            params![input.category_id, i as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}
