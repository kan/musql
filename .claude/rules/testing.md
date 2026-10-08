# テスト方針

## 何をどこで担保するか
- **Rust のユニットテスト**（`just test`）が自動テストの本体。`src-tauri/src/main.rs` 末尾の `mod tests`、`onepassword.rs` の `mod tests`、`docker/discovery.rs` の `mod tests`、`app_log.rs` の `mod tests` に置く。純関数（プロファイルのサニタイズ、同期の merge、AI プロンプト組み立て、op の出力パース、Docker ラベルの正規化、ログの間引きと整形）を対象にする。
- **ドキュメントと実装の乖離**は `src-tauri/src/docs_consistency.rs` が見る（#115。`just check-docs` で単独実行、`just test` にも含まれる）。`main.rs` から `#[cfg(test)]` で取り込むユニットテストにしてある。`tests/` ディレクトリを作って統合テストにしないこと（cargo が本体の `musql.exe` までビルドし、`just dev` の起動中はその exe がロックされていて失敗する）。「実在する」は git に聞く（追跡中、または未追跡で ignore されていないファイル）。ディスクを見ると、生成物や ignore 済みのファイルのせいで手元だけ通って CI で落ちる。開発ノートが挙げるパスとシンボルの実在、ヘルプボタンのアンカー、マニュアルの画像とリンクを照合する。見出しの slug は `ui/manual.js` の `makeSlugger` と同じ規則を持つので、あちらを変えたらテストの `heading_slugs` も合わせる（テストが自動で知らせるのは slug の文字クラスを変えたときだけ。見出しやコードフェンスの判定、`stripInline`、重複時の連番を変えたときは手で合わせる）。
- **依存のライセンス一覧**（`ui/credits.json`、#119）は `src-tauri/src/credits.rs` が作り、照合する。`just credits` で作り直し（`#[ignore]` 付きの `generate`）、`just test` の `credits_match_the_dependencies` が現在の依存と合っているかを見る。対象は、リリースするターゲット（`x86_64-pc-windows-msvc`）と既定の feature で musql から通常の依存でたどれるクレートと、手で並べた vendor の分（`VENDORED`）。**クレートについて照合が比べるのは名前とライセンス名だけで、版は比べない**（版まで比べると、dependabot の PR が毎週かならず落ちてマージできなくなる）。落ちるのは、クレートが増えた、減った、ライセンスが変わったとき。そのときは `just credits` を回して一緒にコミットする。クレートの版は `just bump` が `just credits` を呼んで合わせる。vendor の分は版も比べる（dependabot が触らないため）。`vendored_list_covers_what_is_vendored` は、`ui/lib/` 直下のディレクトリすべてに `VENDORED` の項目とライセンスファイルがあること、CodeMirror の版が vendor のファイルの先頭と合っていることを見る。**リリースするターゲットを増やす、または `default` に入らない feature を足すときは、`credits.rs` の `shipped_crates` も合わせる**（合わせないと、その分の依存が一覧から漏れ、テストは落ちない）。
- **GUI の挙動は自動テストで担保しない**。接続・タブ操作・メニューは実際に `just dev` で触って確認する。コミット前にユーザーの動作確認 OK を取るのはこのため（CLAUDE.md「Git workflow」参照）。
- **UI の静的検査**は Biome（`just lint-ui`）のみ。JS のユニットテストは持たない（Node.js を要求しない方針のため）。

## 外部プロセス・ネットワークに触るテスト
- 実 `op` CLI を叩くテストは `#[ignore]` を付ける。実行は `just test-ignored`。CI では回さない（1Password の認証が要るため）。
- MySQL / SSH / Docker に実接続するテストは、**CI で回るテストとしては書かない**（手元の環境差で落ちるため）。
- 実接続を確かめたいときは `src-tauri/src/verify.rs` の検証テストを使う（#117）。`just verify-ssh` / `just verify-mysql` / `just verify-docker`。`#[ignore]` 付きで、接続先と資格情報は環境変数（`MUSQL_VERIFY_*`。一覧はファイル内のコメント）で渡す。**環境変数が無ければ「skipped」と出して成功する**ので、`just test-ignored` でまとめて回しても落ちない。russh / mysql / bollard を更新したら、GUI を起動する前にこれで確かめる。
  - 検証バイナリ（`src/bin/verify_*.rs`）にしないこと。musql は実行ファイルだけのクレートなので、別の実行ファイルからは `main.rs` の接続処理を呼べない（先にライブラリへの分割が要る）。テストなら `start_ssh_tunnel` や `run_connection_test` をそのまま呼べる。
  - 接続先やパスワードをリポジトリに書かない。出力にもパスワードを出さない。

## 消してはいけないテスト
- `parse_fields_drops_secret_values`（`onepassword.rs`）。`op item get` の応答に含まれる秘密値が WebView に渡らないことを、serde が未知フィールドを捨てる挙動として固定している。`OpField` / `RawField` に `value` を足すと壊れる。
- `sanitized_store_json_*` / `scrub_incoming_profile_*`（`main.rs`）。同期ファイルにシークレットとマシン固有パスを書き出さない保証。
- `sync_guard_*`。古いバージョンのアプリが新しい設定を上書きしないためのガード（#81）。

## CI（`ci.yml`）
- `check` ジョブ（windows-latest）が `just fmt` / `just clippy` / `just test`、`lint-ui` ジョブ（ubuntu-latest）が `just lint-ui`。ローカルの `just check` と同じ内容。
- `audit` ジョブは `cargo audit` を push / PR と週次 cron で回す。起票はしない（warning まで大量に issue 化されるため）。抑止する RUSTSEC は `ignore:` に理由コメント付きで並べる。
