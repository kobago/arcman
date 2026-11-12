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
| `youtube.rs` | URL から動画IDを取り出す、oEmbed の呼び出し、サムネイルのダウンロード計画 |
| `pixiv.rs` | URL から作品IDを取り出す、ajax API の呼び出し、全ページとアイコンのダウンロード計画 |
| `imgur.rs` | URL から投稿IDを取り出す、ページ内の `window.postDataJSON` または OGP の読み取り、全画像・動画とアイコンのダウンロード計画 |
| `reddit.rs` | URL から投稿IDを取り出す、公開 Atom フィードの読み取り、セッションがあるときは `.json` の呼び出し、ギャラリー・DASH 動画・画像のダウンロード計画 |
| `web.rs` | URL の正規化、ページの取得とメタデータの読み取り（scraper、文字コードは encoding_rs）、画像とアイコンのダウンロード計画 |
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
| `ARCMAN_YOUTUBE_OEMBED_URL` | `https://www.youtube.com/oembed` | テスト用。設定画面には出さない |
| `ARCMAN_PIXIV_BASE_URL` | `https://www.pixiv.net` | テスト用。設定画面には出さない |
| `ARCMAN_IMGUR_BASE_URL` | `https://imgur.com` | テスト用。設定画面には出さない |
| `ARCMAN_REDDIT_BASE_URL` | `https://www.reddit.com` | テスト用。設定画面には出さない |

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
| GET | `/api/progress` | `ids`（カンマ区切り）の投稿の状態と、取得中なら進み具合（0〜1。キュー待ちは `null`）。ない投稿は返さない |
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
| `media` | `photo`、`video`（GIF を含む）、`text`（ここまでは x.com、pixiv、imgur、Reddit の投稿）、`pixiv`、`youtube`、`web`、`imgur`、`reddit` |
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
| GET | `/api/settings` | 値と、その出どころ（`saved`、`env`、`default`）。pixiv のセッションは保存済みかどうか（`{saved}`）だけを返す |
| PUT | `/api/settings` | `{fxtwitter_base_url}` |
| PUT | `/api/settings/pixiv-session` | `{session}`。`PHPSESSID=` 付きでも値だけでもよい。空なら削除 |
| PUT | `/api/settings/reddit-session` | `{session}`。`reddit_session=` 付きでも値だけでもよい。空なら削除 |

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

YouTube の動画は、2 の代わりに `GET https://www.youtube.com/oembed?format=json&url=https://www.youtube.com/watch?v={id}` を呼ぶ。`400` と `404` は `not_found`、`401` と `403` は `private` として保存しない（埋め込みを禁止した動画も `401` になるので保存できない）。ワーカーはサムネイルだけを保存する。`maxresdefault.jpg` を試し、なければ oEmbed の `thumbnail_url`（`hqdefault.jpg`）にする。サムネイルは 4:3 に黒帯を足した画像のことがあるので、タイルの縦横比は 16:9（ショートは 9:16）にして切り抜く

pixiv の作品は、2 の代わりに `GET https://www.pixiv.net/ajax/illust/{id}` を呼び、複数ページなら `GET /ajax/illust/{id}/pages` も呼ぶ。設定で pixiv のセッションを保存していれば `Cookie: PHPSESSID=...` を付ける（R-18 の作品はこれがないと画像の URL が返らない）。

- `404` などのエラーは `not_found`、`401`・`403` は `private`、画像の URL がない作品（R-18 で、セッションがないか、そのアカウントで R-18 を表示しない設定）は `login_required` として保存しない。ほかの失敗は `api_unavailable` の失敗として保存する
- スナップショットには読み取った項目（ID、タイトル、キャプションのプレーンテキスト、作者名、アカウント、アイコン、投稿日時、タグ、各ページの原寸画像の URL と大きさ、いいね・ブックマーク・閲覧数）を保存する
- 作品のタグは、スナップショットを保存するときに arcman のタグとして投稿に付ける（同じタグキーのタグがあればそれを使う）
- ワーカーは全ページの原寸画像と作者のアイコンを保存する。`i.pximg.net` は `Referer: https://www.pixiv.net/` がないと `403` を返すので付ける。うごイラは1コマ目の画像だけ

imgur の投稿は、2 の代わりにページを `GET` する。ギャラリーとアルバムのページには `window.postDataJSON` に投稿の JSON が埋め込まれているので、そこからメディア一覧を読む。単体の画像・動画のページは OGP の `og:video`・`og:image` を読む。`i.imgur.com` の直リンクはページを取得せず、拡張子から画像・動画・GIF を決める。

- `404`・`410` は `not_found`、`401`・`403` は `private` として保存しない。ページが返っても投稿のデータがないときも `not_found`
- `media` の各要素の `url`、`width`、`height`、`metadata.duration` を保存する。`type` が `video` のものは mp4 を保存し、`metadata.is_animated` かつ音声なしなら GIF として扱う
- 動画のサムネイルは `{id}h.jpg` を使う
- スナップショットには読み取った項目（ID、タイトル、説明文、アカウント名とアイコン、投稿日時、閲覧数、いいね数、コメント数、メディア一覧）を保存する

Reddit の投稿は、Reddit が公開している Atom フィード `GET {base}/comments/{id}.rss` を読む。2026年に未ログインの `.json` が 403 で拒否されるようになったため、RSS を既定の経路にしている。設定画面で Reddit のセッション（`reddit_session`）を保存しているときは、先に `GET {base}/comments/{id}.json?raw_json=1&limit=1` を `Cookie: reddit_session=...; over18=1` 付きで呼び、ギャラリーや動画を含む完全な投稿を読む。`.json` が拒否されたら RSS に戻る。`i.redd.it`・`v.redd.it` などの直リンクはページを取得せず、拡張子から画像・動画を決める。

- RSS の `404` は `not_found`、`401`・`403` は `private` として保存しない。`429` は API に接続できない失敗として保存し、再試行できる
- RSS の最初のエントリ（`t3_`）から、ID、タイトル、本文、投稿者、サブレディット、投稿日時を読む。`[link]` が `i.redd.it` の画像なら保存し、`v.redd.it` の動画なら `DASHPlaylist.mpd` から最高画質の mp4 を選ぶ。ギャラリーは RSS に先頭画像のプレビューしかないため、セッションがないときはその1枚を保存する
- 本文中の画像（`div.md` の中の `i.redd.it`・`preview.redd.it` のリンクや `img`）も保存する。プレビューの URL は `width=1080` に直して保存する
- ホスト動画の音声は別トラックなので結合しない（音声なしの mp4 になる）
- スナップショットには RSS または JSON から読み取った項目（タイトル、本文、投稿者、サブレディット、パーマリンク、投稿日時、スコア、コメント数、メディア一覧）を保存する

Web ページは、2 の代わりにページを `GET` する（20秒で打ち切り、HTML は先頭 1MB まで読む）。

- `404` と `410` は `not_found` として保存しない。ほかの失敗は `page_unavailable` の失敗として保存する
- 文字コードは `Content-Type` の `charset`、なければ先頭の `<meta>` の宣言、なければ UTF-8 で読む
- `og:*`、`twitter:*`、`<title>`、`description`、`<link rel="apple-touch-icon">`・`<link rel="icon">`（なければ `/favicon.ico`）を読み、相対 URL はリダイレクト後の URL で解決する。画像の URL を直接保存したときは、その画像を OGP 画像として扱う
- スナップショットには読み取ったメタデータ（`url`、`title`、`description`、`site_name`、`image`、`icon`、`lang`）を保存する
- 画像とアイコンは取れなくても完了にする（`Content-Type` が `image/` でないものも保存しない）。画像の大きさがわからなければ 1200×630 として扱う

ページの取得はログインした利用者の操作で行うので、LAN 内のアドレスも制限しない

ファイルは `media/{post_id}/{role}-{position}.{ext}` に `.part` で書いてから名前を変える。

## 5. 詳細のレスポンス

スナップショットを型に読み直し、リモートの URL を保存したファイルの URL（`/media/...`）に置き換えて返す。YouTube の動画は `status` の代わりに `video`（ID、URL、ショートか、タイトル、チャンネル、サムネイル）を、Web ページは `page`（URL、タイトル、説明文、サイト名、ドメイン、画像、アイコン）を返す。pixiv の作品、imgur と Reddit の投稿は x.com の投稿と同じ `status` の形で返す（本文はタイトルと説明文・本文、統計は pixiv がいいね・ブックマーク・閲覧数、imgur がいいね・閲覧数・コメント数、Reddit がスコア・コメント数）。X記事は本文のブロックをそのまま返し、画像の URL の置き換え表（`local_urls`）を添える。

## 6. SPA の配信

- `frontend/dist` を rust-embed で実行ファイルに埋め込み、1つの実行ファイルで配信する
- `/api/` と `/media/` 以外のパスで該当ファイルがなければ `index.html` を返す（クライアント側のルーティングのため）
- `assets/` 以下はファイル名にハッシュが付くので `immutable` でキャッシュし、`index.html` は `no-cache` にする
- `build.rs` で `frontend/dist` の変更を監視し、SPA を作り直したらバックエンドも再コンパイルされるようにする
- `frontend/dist` がない状態でもビルドでき、そのときは画面の代わりに「frontend is not built」を返す
