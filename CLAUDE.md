# muSQL project notes (for agents)

Windows向け MySQL クライアント。Tauri v2 + Rust backend + 静的 UI（`ui/` の HTML/JS/CSS、Node.js 不要）。SSH 踏み台対応（`russh`）。

## 詳細ルール（触る領域のものを読む）

このファイルには全体像と運用だけを置き、領域別の実装メモは `.claude/rules/` に分けてある。**`@import` していないので、その領域を触るときに自分で読むこと。**

- `.claude/rules/rust.md` … Rust 実装ルール（秘密情報の扱い・接続とキャンセル・Docker 連携・ウィンドウ管理・設定同期）
- `.claude/rules/frontend.md` … UI 実装ルール（画面構成・機能ごとの実装メモ・localStorage keys・Limits）
- `.claude/rules/testing.md` … テスト方針（何を自動テストで担保し、何を手動確認に寄せるか）

## Docs の役割分担
- `README.md` = 人間向け（概要・ダウンロード・機能ハイライト）。`docs/manual/` = エンドユーザーマニュアル（機能別・日本語）。このファイル = 開発/エージェント向け内部情報。重複を避け、各々の役割に閉じる。
- **ユーザー向け挙動を変えたら `docs/manual/` の該当ページも更新する**（画像は `docs/manual/img/`、撮影リストは `docs/manual/img/README.md`）。マニュアルは pike 構成に倣い、見出しアンカーは GitHub slug。校正は下の「ドキュメント校正ルール」に従う。

### ドキュメント校正ルール

**日本語のユーザー向けドキュメントを更新・追加したら、コミット前に校正する。**

- 対象：`README.md` / `docs/manual/` 配下 / `CHANGELOG.md`（リリース時に足す新しいセクション）
- 対象外：`CLAUDE.md`（読み手が開発者の密な技術メモ）、英語で書く `SECURITY.md` と `PRIVACY.md`

1. **textlint（機械チェック）** を `just lint-docs` で回し、今回書いた箇所の指摘を 0 にする（既存の指摘は 4 を参照）。実体は npx なのでリポジトリに textlint は入れない。対象ファイルとルールプリセットは justfile の `lint-docs` レシピを正とする。

2. **`japanese-tech-writing` スキル**（判断ベース）で、textlint が拾えない空句・冗長・演出・論証を点検する。

3. **守る表記規約**:
   - 箇条書きの太字ラベルの区切りは**全角コロン**で `**用語**：説明` と書く。半角コロン `:` は `no-ai-list-formatting` に触れるため使わない
   - 地の文・見出しで **em ダッシュ `—` を使わない**（全角コロンか句読点にする）
   - 誇張語（「大幅に」等）・LLM 空句（「重要なのは」「正面から」「多角的」等）を使わない
   - 二重助詞・一文内の過多カンマ（4 個以上）を避ける

4. **据え置いてよい指摘**:
   - `no-mix-dearu-desumasu`（本文の「です・ます」と箇条書き・表セルの体言止めの混在）と、列挙が主因の `sentence-length`。マニュアルとして自然なので無理に潰さない
   - **CHANGELOG の過去セクション**。出荷済みの記録なので、表記の一括正規化以外は書き換えない。校正するのはそのリリースで足す節だけ
   - 誤検出の常連が 2 つある。UI 名やエスケープシーケンスに出るリテラルの `?`（`no-exclamation-question-mark`）と、行を折り返した括弧（`no-unmatched-pair` が閉じ括弧を見失う）

5. 見出しを変えたら、ページ内アンカー（`](#...)`）と `data-manual` 属性のアンカーとの整合を確認する（「アプリ内マニュアル」参照）。

## How to run
- 開発タスクの入口は `justfile`。`just` でレシピ一覧、`just dev` で起動、`just check` でコミット前チェック一式（fmt / clippy / test / UI lint）、`just bump X.Y.Z` でバージョン更新。`ci.yml` の各ステップも同じレシピを呼ぶ（ステップ名は残したまま中身だけ just に寄せてあるので、失敗箇所の粒度は従来どおり）。
  - just は cargo の薄いファサードで、cargo から直接叩いても同じ。`cargo dev` の alias（`.cargo/config.toml`）はそのまま残してある。
  - justfile 先頭の `set windows-shell` は必須。just の Windows 既定シェル `sh -c` は PATH に無く、PATH 上の `bash` は WSL ランチャ（`C:\Windows\System32\bash.exe`）で Windows 側の cargo / tauri が見えないため、Git Bash を明示している。Git を別の場所に入れている環境は `just --shell <bash へのパス>` で上書きする。
  - `release.yml` は just を経由しない。ビルドは `tauri-action` と PowerShell スクリプトが主体で、レシピに寄せても重複が減らないため。
- `cargo dev`（= `just dev`）で起動（リポジトリ直下 / src-tauri のどちらからでも可。`.cargo/config.toml` の alias で `cargo tauri dev --config tauri.dev.conf.json` に展開される）。dev config は identifier を `...musql.debug` に上書きし、インストール版とウィンドウ状態（`tauri-plugin-window-state`）・アプリデータを分離する。
- `cargo check` / `cargo test` / `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`。
- UI (`ui/*.js`) の lint: `just lint-ui`（= `npx @biomejs/biome@2.4.10 lint --error-on-warnings`。Biome、package.json 不要。設定は `biome.json`、`ui/lib/**` の vendor は除外）。CI は `setup-biome` が入れた `biome` を使うため、レシピは PATH にあればそちらを優先し、無ければ npx にフォールバックする。

## Architecture
- Rust は `src-tauri/src/main.rs` に全ロジック集約。Docker 連携だけ `src-tauri/src/docker/`（`discovery.rs`, `tunnel.rs`）、1Password 連携は `src-tauri/src/onepassword.rs`。
- UI は `ui/` の 3 画面（main = プロファイル一覧 / settings = プロファイル編集 / query = DB エクスプローラ）。ウィンドウは `tauri.conf.json` で事前定義し、show/hide パターンで管理する。
- Cargo feature で機能を分離：`self-updater`（デフォルト有効、Store ビルドでは無効）、`docker`（デフォルト有効、`bollard` / `futures-util` 依存を分離）。

## Build variants
- **Standalone（デフォルト）**: `cargo tauri build` — NSIS インストーラ + セルフアップデータ付き。
- **Store**: `cargo tauri build --config tauri.store.conf.json -- --no-default-features --features docker` — アップデータ無効・Docker有効。Store 用 EXE を生成。
- **Store dev 確認**: `cargo tauri dev --config src-tauri/tauri.store.conf.json -- --no-default-features --features docker`（要 Developer Command Prompt / RC.EXE in PATH）。
- Store 用アイコン: `src-tauri/icons/Square44x44Logo.png`, `Square150x150Logo.png`, `StoreLogo.png`。
- CI: `release.yml` の `build-store` ジョブが Store EXE と MSIX をリリースにアップロード。MSIX は `store/AppxManifest.xml` + `store/build-msix.ps1` で生成する（未署名。Microsoft 側で署名される）。
- Partner Center Identity: `58967CommunitylinksInc.muSQL` / `CN=46BBEF28-6777-4EF5-AD2C-F9AD9123AA82`。

## コミット前チェック

**コミットの前は、変更の規模に応じて次を実行し、指摘を反映してからコミットする。**

| 変更の規模 | 実行するもの |
|---|---|
| ある程度の規模の実装・修正 | `/code-review` → `simplify` → `just check` |
| 軽微なコード修正 | `simplify` → `just check`（自明な 1 行修正などは直接コミットしてもよい） |
| ドキュメントのみ（`README.md` / `docs/manual/` / `CHANGELOG.md`） | 「ドキュメント校正ルール」の校正 |
| バージョン bump のみ | 何も要らない |

- **順序を守る**。`/code-review`（バグ探索）で挙がったものを直してから `simplify`（再利用・単純化・効率・抽象度の品質整理）を回す。simplify はバグを探さないので、先に回しても直すべきコードを整えるだけになる
- どちらもコードを書き換えるため、必ず**ユーザーの動作確認より前**に実行する（ユーザーは適用後のコードを試す）
- `/code-review` はユーザーがコマンドを打つこともある

## Git workflow
- PR 運用はしていない。修正は **main に直接コミット** する。
- コミット前にユーザーの動作確認 OK を取る。lint / test が通っただけでコミットしない（GUI アプリなので実際に触らないと分からない）。
- ただしエージェント（Claude Code 等）は **push しない**。コミットまでに留め、push の判断はユーザーに委ねる。ユーザーが内容を確認後、自身で `git push origin main` を実行する。
  - **例外: リリース依頼**。「リリースして」は bump コミットだけでなく、**push・タグ作成と push・ワークフロー完了待ち・リリースノート記載までの一括依頼**。個別に push の確認を取らず「Release procedure」を最後まで完遂する。
- ブランチを切って PR を作る運用は不要。

## Release procedure

リリース依頼を受けたら、以下を最後まで通しで実行する（push の個別確認は不要。Git workflow の例外規定）。バージョン番号は変更内容から判断する（新機能ならマイナー、修正のみならパッチ）が、**判断に迷う場合と、ユーザーが番号を指定していない大きめの変更ではユーザーに確認する**。

### 1. バージョンと CHANGELOG

- `CHANGELOG.md` の先頭に新セクション（日付・Added/Changed/Fixed/Security・末尾の比較リンク）
- `just bump X.Y.Z` で `src-tauri/Cargo.toml` と `src-tauri/tauri.conf.json` の `version` を更新し、`cargo check` で `Cargo.lock` の `musql` エントリまで追従させる。**忘れると lockfile drift が残り、後から同期コミットが必要になる**

`store/AppxManifest.xml` は `Version="{{VERSION}}"` のプレースホルダで、CI がタグから流し込むため編集不要。

### 2. 検証してコミット

`just check`（fmt / clippy / test / UI lint）を通してから:

```
git add CHANGELOG.md src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "Bump version to X.Y.Z"
```

### 3. push とタグ

```
git push origin main
git tag vX.Y.Z && git push origin vX.Y.Z
```

**タグを打つ前に必ずバージョンを更新すること** — `tauri-action` は `tauri.conf.json` の `version` をアセット名に埋め込むため、ずれると `latest.json` の指す先と実ファイル名が食い違う。

タグを打ち直す場合: `git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z` → 修正後に再タグ。

### 4. ワークフロー完了待ち

タグ push で `Release` が起動する。ジョブは 2 つあり**両方**待つこと。

- `build` — NSIS インストーラ + `latest.json`（セルフアップデータ用）
- `build-store` — Store 用 EXE + MSIX を同じ Release に追加アップロード

Windows のフルビルドで 10〜20 分かかる。`until [ "$(gh run view <id> --json status --jq .status)" = "completed" ]; do sleep 30; done` をバックグラウンドで回して待つ（ポーリングを前景で回さない）。

### 5. アセット確認とリリースノート

`releaseDraft: false` なので**タグ push の時点で Release は公開される**（pike のドラフト運用とは異なる）。`releaseBody` は "See the assets below to download and install." の固定文なので、完了後に CHANGELOG の内容で上書きする:

```
gh release view vX.Y.Z --json assets --jq '.assets[].name'
gh release edit vX.Y.Z --notes "..."
```

**`latest.json` が添付されているか必ず確認する。** これが無いとセルフアップデータが黙って壊れる（`tauri-action` v1.0.0 で `includeUpdaterJson` → `uploadUpdaterJson` にリネームされた経緯があり、設定漏れが起きやすい）。期待されるアセットは NSIS インストーラ (`.exe` / `.exe.sig`) 、`latest.json`、`muSQL-store-x64.exe`、`muSQL-store-x64.msix`。

`TAURI_SIGNING_PRIVATE_KEY` / `..._PASSWORD` が GitHub Secrets に必要（未署名ビルドは updater の検証に失敗する）。

## 計画

未着手の計画は GitHub issue で管理する（このファイルには置かない）。`gh issue list --label enhancement` で見る。
