# arcman バックエンド設計（MVP）

[ドメインモデル](domain-model.md) を Rust と axum で実装するための設計。

## 1. 構成

| 項目 | 選択 | 理由 |
|---|---|---|
| Web フレームワーク | axum 0.8 | 指定 |
| DB | SQLite（rusqlite、bundled） | 利用者がひとりで、`~/.arcman/` をコピーするだけでバックアップできる |
| DB アクセス | `Mutex<Connection>` を1本共有 | 書き込みは1人分で、ロックの競合が問題にならない |
| HTTP クライアント | reqwest（rustls） | fxtwitter API とメディアのダウンロード |
| パスワード | argon2（Argon2id、PHC 文字列） | 要件 |
| セッション | ランダムな32バイトのトークンを Cookie に入れ、DB には SHA-256 ハッシュだけを保存 | DB が漏れてもセッションを乗っ取れない |
| 時刻 | SQLite の `strftime` で UTC の ISO 8601 文字列にそろえる | 時刻ライブラリを増やさない |
| CLI | `std::env::args` を自前で解釈 | サブコマンドが2つだけ |

### クレートの構成

`arcman` は lib と bin の2つのターゲットを持つ。結合テストは lib の `app()` を使う。

| モジュール | 役割 |
|---|---|
| `main.rs` | CLI（`serve`、`set-password`） |
| `lib.rs` | `Config`、`AppState`、ルーター、SPA の配信 |
| `db.rs` | 接続、スキーマ、マイグレーション（`PRAGMA user_version` で管理） |
| `error.rs` | API エラー（HTTP ステータスとエラーコード） |
| `auth.rs` | パスワード、セッション、ログイン失敗の制限、認証ミドルウェア |
| `fxtwitter.rs` | URL から投稿IDを取り出す、API 呼び出し、レスポンスの型 |
| `archive.rs` | メディアのダウンロード計画と、1件ずつ処理するワーカー |
| `posts.rs` | 投稿の保存、タイル、詳細、削除、再試行、フォルダの移動、タグの付け替え |
| `organize.rs` | カテゴリとフォルダ |
| `tags.rs` | タグキーの正規化、タグの管理 |
| `settings.rs` | 設定の解決（保存した値 → 環境変数 → 既定値） |

## 2. 設定と起動

| 環境変数 | 既定値 | 意味 |
|---|---|---|
| `ARCMAN_HOME` | `~/.arcman` | データと設定の置き場所 |
| `ARCMAN_ADDR` | `127.0.0.1:3000` | 待ち受けアドレス。`--addr` が優先 |
| `ARCMAN_FXTWITTER_BASE_URL` | `https://api.fxtwitter.com` | 設定画面の初期値。画面で保存した値が優先 |

```
arcman serve [--addr HOST:PORT]   サーバーを起動する（例: Tailscale 越しなら --addr 0.0.0.0:3000）
arcman set-password               パスワードを設定する（端末なら2回入力。パイプなら標準入力の1行）
```

- 待ち受けアドレスは起動前に決まっている必要があり、Web の設定画面には置かない（変えた結果アクセスできなくなっても、画面から戻せないため）
- 起動時に決める項目は今は環境変数と起動オプションで足りるので、設定ファイルは作らない。起動時の項目が増えたら `~/.arcman/` の設定ファイルに対応する。Web の設定画面と同じ項目は置かない
- Cookie は `HttpOnly; SameSite=Strict`。リクエストに `X-Forwarded-Proto: https` があれば `Secure` も付ける（TLS は前段のリバースプロキシで終端する想定）
- ログインに5回続けて失敗したら、60秒間ログインを受け付けない（`429`）

## 3. API

認証が必要なものは、Cookie のセッションがなければ `401`。エラーは `{"error": "<code>"}` で返し、文言はフロントエンドが表示言語に合わせて作る。

### 認証

| メソッド | パス | 内容 |
|---|---|---|
| POST | `/api/login` | `{password}`。成功で `204` と Cookie |
| POST | `/api/logout` | この端末のセッションを消す |
| POST | `/api/logout-all` | すべてのセッションを消す |
| GET | `/api/session` | ログイン中なら `204` |

### 投稿

| メソッド | パス | 内容 |
|---|---|---|
| POST | `/api/posts` | `{urls, folder_id?}`。URL ごとの結果を返す: `created`、`duplicate`、`invalid_url`、`unavailable`（`not_found` か `private`） |
| GET | `/api/tiles` | ギャラリーのタイル。下の絞り込み条件と `cursor`、`limit`（投稿の件数） |
| GET | `/api/posts/{id}` | 詳細 |
| PATCH | `/api/posts/{id}` | `{folder_id}`（`null` で未分類） |
| PUT | `/api/posts/{id}/tags` | `{names}`。タグの集合を置き換える。ないタグは作る |
| POST | `/api/posts/{id}/retry` | 失敗した投稿を再試行する |
| DELETE | `/api/posts/{id}` | 削除する |
| GET | `/api/authors` | 絞り込み用の投稿者の一覧 |
| GET | `/media/{post_id}/{file}` | 保存したメディアファイル |
| GET | それ以外 | SPA（`index.html` と `assets/`）。ログイン不要 |

`/api/tiles` の絞り込み条件:

| パラメータ | 値 |
|---|---|
| `folder` | `all`（既定）、`unsorted`、フォルダの ID |
| `q` | 本文とタグ名の部分一致 |
| `tags` | タグ ID のカンマ区切り（AND） |
| `from`、`to` | 保存日時の範囲（ISO 8601。`to` は含まない） |
| `media` | `photo`、`video`（GIF を含む）、`text` |
| `author` | 投稿者のスクリーンネーム |
| `order` | `desc`（既定）、`asc` |

ページは投稿単位で区切るので、1つの投稿のタイルがページをまたぐことはない。`next_cursor` を次のリクエストの `cursor` に渡す。

### カテゴリとフォルダ

| メソッド | パス | 内容 |
|---|---|---|
| GET | `/api/sidebar` | カテゴリとフォルダを並び順で返す |
| POST | `/api/categories` | `{name}` |
| PATCH | `/api/categories/{id}` | `{name}` |
| DELETE | `/api/categories/{id}` | 中のフォルダはカテゴリなしになる |
| PUT | `/api/categories/order` | `{ids}` |
| POST | `/api/folders` | `{name, category_id?}` |
| GET | `/api/folders/{id}` | 削除の確認用に投稿の件数を含める |
| PATCH | `/api/folders/{id}` | `{name}` |
| DELETE | `/api/folders/{id}` | 中の投稿も削除する |
| PUT | `/api/folders/order` | `{category_id, ids}`。並び順とカテゴリをまとめて置き換える |

### タグ

| メソッド | パス | 内容 |
|---|---|---|
| GET | `/api/tags` | `sort=name|count`。件数つき |
| PATCH | `/api/tags/{id}` | `{name?, color?}`。名前が既存のタグと衝突したら `409` |
| DELETE | `/api/tags/{id}` | 削除する |

### 設定

| メソッド | パス | 内容 |
|---|---|---|
| GET | `/api/settings` | 値と、その出どころ（`saved`、`env`、`default`） |
| PUT | `/api/settings` | `{fxtwitter_base_url}` |

## 4. 保存の流れ

1. `POST /api/posts` で、URL ごとに投稿IDを取り出す。重複していれば既存の ID を返す
2. `GET {base}/2/status/{id}?lang=ja` を呼ぶ。`lang=ja` で日本語訳も受け取る
   - `404` と `401` は保存しない（`unavailable`）
   - 接続できない、または `5xx` なら、失敗の投稿として保存する（再試行できる）
   - `200` ならスナップショットと取り出したカラムを保存し、取得中にしてワーカーに渡す
3. ワーカーはスナップショットからダウンロード計画を作り、1件ずつ保存する
   - 画像は `url`（`?name=orig` 付き）
   - 動画と GIF は `formats` の mp4 のうち最高ビットレートのもの。サムネイルと長さ（秒）も保存する
   - 投稿者アイコン、引用元のメディアとアイコン、リンクカードの画像
   - X記事は、`article` の中にある `pbs.twimg.com` の画像 URL をすべて保存する
4. すべて保存できたら完了、1つでも失敗したら失敗にする。起動時に取得中の投稿をワーカーに渡し直す

ファイルは `media/{post_id}/{role}-{position}.{ext}` に `.part` で書いてから名前を変える。

## 5. 詳細のレスポンス

スナップショットを型に読み直し、リモートの URL を保存したファイルの URL（`/media/...`）に置き換えて返す。X記事は本文のブロックをそのまま返し、画像の URL の置き換え表（`local_urls`）を添える。

## 6. SPA の配信

- `frontend/dist` を rust-embed で実行ファイルに埋め込み、1つの実行ファイルで配信する
- `/api/` と `/media/` 以外のパスで該当ファイルがなければ `index.html` を返す（クライアント側のルーティングのため）
- `assets/` 以下はファイル名にハッシュが付くので `immutable` でキャッシュし、`index.html` は `no-cache` にする
- `build.rs` で `frontend/dist` の変更を監視し、SPA を作り直したらバックエンドも再コンパイルされるようにする
- `frontend/dist` がない状態でもビルドでき、そのときは画面の代わりに「frontend is not built」を返す
