# ARCMAN

x.com の投稿をブックマークし、画像や動画ごと手元に保存するセルフホストのアーカイブマネージャー。

## ビルドと起動

```sh
cd frontend && bun install && bun run build
cd ../backend && cargo build --release

printf 'your-password\n' | ./target/release/arcman set-password
./target/release/arcman serve --addr 0.0.0.0:3000
```

データと設定は `~/.arcman/` に保存される（`ARCMAN_HOME` で変更可）。このディレクトリをコピーすればバックアップになる。

## 開発

```sh
cd backend && cargo run -- serve          # API（127.0.0.1:3000）
cd frontend && bun run dev                # 画面（/api と /media は 3000 に転送）
```

| コマンド | 内容 |
|---|---|
| `cargo test`（backend） | API のテスト。`cargo test -- --ignored` で本物の fxtwitter を使うテストも動く |
| `bun run test`（frontend） | Vitest |
| `bun run e2e`（frontend） | Playwright。初回は `bunx playwright install chromium` が必要 |

## ドキュメント

- [要件定義書](docs/requirements.md)
- [ドメインモデル](docs/domain-model.md)
- [バックエンド設計](docs/backend-design.md)
- [フロントエンド設計](docs/frontend-design.md)
