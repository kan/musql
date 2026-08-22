# muSQL のタスクランナー。`just` で一覧、`just check` でコミット前チェック一式。
#
# レシピは cargo の薄いファサードにしてある（`cargo dev` の alias や CI の慣習を
# 壊さないため）。cargo から直接叩いても just から叩いても同じことが起きる。

# レシピは Git Bash で走らせる。just の Windows 既定シェル（sh -c）は PATH に
# 存在せず、PATH 上の bash は WSL ランチャ（C:\Windows\System32\bash.exe）で、
# そちらには Windows 側の cargo / tauri が無い。Git を別の場所に入れている場合は
# `just --shell <bash へのパス>` で上書きする（GitHub の windows runner は
# このパスで合っている）。
set windows-shell := ["C:/Program Files/Git/bin/bash.exe", "-cu"]

# Biome は package.json を持たないので npx で都度取る。CI は setup-biome が入れた
# biome を使うため、PATH にあればそちらを優先する（lint-ui 参照）。
biome_version := "2.4.10"

# レシピ一覧を出す
default:
    @just --list --unsorted

# --- 開発 ---

# 開発版を起動する（identifier は .debug なのでインストール版と共存できる）
dev:
    cargo dev

# インストーラまで含めた本番ビルド（NSIS + セルフアップデータ）
[working-directory('src-tauri')]
build:
    cargo tauri build

# Store 用ビルド（アップデータ無効・Docker 有効）
[working-directory('src-tauri')]
build-store:
    cargo tauri build --config tauri.store.conf.json -- --no-default-features --features docker

# --- コミット前チェック ---

# コミット前チェック一式（fmt / clippy / test / UI lint）
check: fmt clippy test lint-ui

# rustfmt の検査（書き換えはしない）
[working-directory('src-tauri')]
fmt:
    cargo fmt --check

# rustfmt で書き換える
[working-directory('src-tauri')]
fmt-fix:
    cargo fmt

# clippy（警告もエラー扱い）
[working-directory('src-tauri')]
clippy:
    cargo clippy --all-targets -- -D warnings

# Rust のユニットテスト
[working-directory('src-tauri')]
test:
    cargo test

# 実 CLI（op コマンド）を叩く #[ignore] 付きテストも含めて回す
[working-directory('src-tauri')]
test-ignored:
    cargo test -- --ignored

# 静的 UI（ui/*.js）の Biome lint
lint-ui:
    if command -v biome >/dev/null 2>&1; then biome lint --error-on-warnings; else npx --yes @biomejs/biome@{{biome_version}} lint --error-on-warnings; fi

# 日本語ドキュメントの textlint（CLAUDE.md の「ドキュメント校正ルール」参照）
lint-docs:
    npx --yes --package textlint \
      --package textlint-rule-preset-ai-writing \
      --package textlint-rule-preset-ja-technical-writing \
      -- textlint --rule preset-ai-writing --rule preset-ja-technical-writing \
      README.md docs/manual/*.md CHANGELOG.md

# --- 監査（ci.yml の audit ジョブと同じ内容） ---

# 依存の脆弱性を見る（cargo install cargo-audit が要る）
[working-directory('src-tauri')]
audit:
    cargo audit

# --- リリース ---

# バージョンを上げる（Cargo.toml / tauri.conf.json + Cargo.lock）。CHANGELOG は手で書く
bump VERSION:
    sed -i -E '0,/^version = /s|^version = ".*"|version = "{{VERSION}}"|' src-tauri/Cargo.toml
    sed -i -E '0,/"version":/s|"version": ".*"|"version": "{{VERSION}}"|' src-tauri/tauri.conf.json
    cd src-tauri && cargo check --quiet
    @echo 'CHANGELOG.md の新セクションは手で書くこと'
