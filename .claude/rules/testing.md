# テスト方針

## 何をどこで担保するか
- **Rust のユニットテスト**（`just test`）が自動テストの本体。`src-tauri/src/main.rs` 末尾の `mod tests` と `onepassword.rs` の `mod tests` に置く。純関数（プロファイルのサニタイズ、同期の merge、AI プロンプト組み立て、op の出力パース）を対象にする。
- **GUI の挙動は自動テストで担保しない**。接続・タブ操作・メニューは実際に `just dev` で触って確認する。コミット前にユーザーの動作確認 OK を取るのはこのため（CLAUDE.md「Git workflow」参照）。
- **UI の静的検査**は Biome（`just lint-ui`）のみ。JS のユニットテストは持たない（Node.js を要求しない方針のため）。

## 外部プロセス・ネットワークに触るテスト
- 実 `op` CLI を叩くテストは `#[ignore]` を付ける。実行は `just test-ignored`。CI では回さない（1Password の認証が要るため）。
- MySQL / SSH / Docker に実接続するテストは書かない。手元の環境差で落ちるため、手動確認に寄せる。

## 消してはいけないテスト
- `parse_fields_drops_secret_values`（`onepassword.rs`）。`op item get` の応答に含まれる秘密値が WebView に渡らないことを、serde が未知フィールドを捨てる挙動として固定している。`OpField` / `RawField` に `value` を足すと壊れる。
- `sanitized_store_json_*` / `scrub_incoming_profile_*`（`main.rs`）。同期ファイルにシークレットとマシン固有パスを書き出さない保証。
- `sync_guard_*`。古いバージョンのアプリが新しい設定を上書きしないためのガード（#81）。

## CI（`ci.yml`）
- `check` ジョブ（windows-latest）が `just fmt` / `just clippy` / `just test`、`lint-ui` ジョブ（ubuntu-latest）が `just lint-ui`。ローカルの `just check` と同じ内容。
- `audit` ジョブは `cargo audit` を push / PR と週次 cron で回す。起票はしない（warning まで大量に issue 化されるため）。抑止する RUSTSEC は `ignore:` に理由コメント付きで並べる。
