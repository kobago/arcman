# arcman フロントエンド設計（MVP）

[要件定義書](requirements.md) と画面デザイン（https://claude.ai/artifact/Xv2NnYrrwYBBXG6pMVbYEL）を、[バックエンド](backend-design.md) の API の上に実装するための設計。

## 1. 構成

| 項目 | 選択 |
|---|---|
| ビルド | Vite、TypeScript |
| UI | React 19 |
| ルーティング | React Router（宣言的な `<Routes>`） |
| サーバー状態 | TanStack Query（一覧は `useInfiniteQuery`） |
| スタイル | 素の CSS（`src/index.css`）。色はすべて CSS 変数で、ダークが既定、ライトは `prefers-color-scheme` と `data-theme` で切り替える |
| 多言語 | 自前の辞書（`src/i18n.ts`）。日本語と英語 |
| テスト | ロジックは Vitest、画面の流れは Playwright |
| パッケージ管理 | Bun |

## 2. 画面とURL

| URL | 画面 |
|---|---|
| `/login` | ログイン |
| `/` | ギャラリー。絞り込み条件はクエリに入れる（`folder`、`q`、`tags`、`from`、`to`、`media`、`author`、`order`） |
| `/posts/{post_id}/{index}` | 詳細。ギャラリーの子ルートとして重ねて表示し、クエリはギャラリーと共有する |
| `/tags` | タグの管理 |
| `/settings` | 設定 |

- 詳細画面の前後移動は URL を置き換える（`replace`）。ブラウザの「戻る」1回でギャラリーに戻る
- 期間の `to` は画面では「その日を含む」、API には翌日0時（ローカル時刻）を渡す

## 3. 主な部品

| ファイル | 役割 |
|---|---|
| `layout.ts` | タイルの行詰め（高さを揃えて幅を埋める。最後の行は伸ばさない） |
| `gallerySize.ts` | タイルの大きさ（5段階の行の高さ）。ブラウザに保存し、URL には入れない |
| `filters.ts` | URL と絞り込み条件の相互変換、API へのクエリ |
| `format.ts` | 時間や日時の表示、タグキーの正規化（サーバーと同じ規則）、スワイプ方向の判定 |
| `components/Sidebar.tsx` | カテゴリとフォルダ。作成、名前の変更、削除、ドラッグでの並べ替え（HTML5 のドラッグ＆ドロップ） |
| `components/SaveBox.tsx` | URL の保存。PC は上部の入力欄、スマホは「＋」で開くシート |
| `components/SaveToasts.tsx` | 保存の結果を右下に通知する。保存とダウンロードの進捗バーを出し、すべて済んだら5秒で消す。エラーがあれば閉じるまで残す |
| `components/Gallery.tsx` | タイルの一覧と無限スクロール |
| `components/Detail.tsx` | 詳細画面。画像は余白なしで表示し、閉じる・保存・前後の矢印は画像に重ねる。PC は右に投稿の内容、スマホは写真アプリ風の下からのシート |

## 4. 振る舞い

- 取得中のタイルがある間は、一覧を3秒ごとに取り直す
- API が 401 を返したら、ログイン画面に移る
- 表示言語とテーマはブラウザの `localStorage` に保存する（サーバーには送らない）

## 5. テスト

- `bun run test`: Vitest。行詰め、URL の変換、表示の整形、スワイプ判定
- `bun run e2e`: Playwright。偽の fxtwitter サーバー（`e2e/fake-fxtwitter.ts`）、一時ディレクトリで起動した本物のバックエンド、Vite の開発サーバーを立ち上げて、ログイン、保存、詳細、フォルダ、タグ、スマホ表示を通しで確かめる
