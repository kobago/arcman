# ARCMAN

x.com の投稿をブックマークし、画像や動画ごと手元に保存するセルフホストのアーカイブマネージャー。YouTube の動画と一般の Web ページも、リンクとサムネイル付きでブックマークできる。

## Linux サーバーで常駐させる

サーバーに Rust（rustup）と Bun を入れてから、リポジトリで次を実行する。

```sh
make install                                  # 画面をビルドして ~/.cargo/bin/arcman に入れる
printf 'your-password\n' | arcman set-password
make service                                  # systemd のユーザーサービスとして登録・起動（0.0.0.0:3000）
sudo loginctl enable-linger $USER             # 初回だけ。ログインしていなくても起動時に動くようにする
```

待ち受けアドレスは `make service ADDR=100.x.y.z:3000` のように変えられる（Tailscale の IP だけで待ち受けるときなど）。

| コマンド | 内容 |
|---|---|
| `git pull && make install && make restart` | 更新 |
| `make status` / `make logs` | 状態の確認 / ログを追う |
| `make uninstall` | サービスを止めて登録解除（`~/.arcman` のデータは残る） |

データと設定は `~/.arcman/` に保存される（`ARCMAN_HOME` で変更可）。このディレクトリをコピーすればバックアップになる。

## 手動でビルドして起動する

```sh
cd frontend && bun install && bun run build
cd ../backend && cargo build --release
./target/release/arcman serve --addr 0.0.0.0:3000
```

## 開発

```sh
cd backend && cargo run -- serve          # API（127.0.0.1:3000）
cd frontend && bun run dev                # 画面（/api と /media は 3000 に転送）
```

| コマンド | 内容 |
|---|---|
| `cargo test`（backend） | API のテスト。`cargo test -- --ignored` で本物の fxtwitter、YouTube、Web ページを使うテストも動く |
| `bun run test`（frontend） | Vitest |
| `bun run e2e`（frontend） | Playwright。初回は `bunx playwright install chromium` が必要 |
| `make test` | 上の3つをまとめて実行 |
| `make test` | 上の3つをまとめて実行 |

## ドキュメント

- [要件定義書](docs/requirements.md)
- [ドメインモデル](docs/domain-model.md)
- [バックエンド設計](docs/backend-design.md)
- [フロントエンド設計](docs/frontend-design.md)
