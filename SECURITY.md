# Security Policy

## Supported Versions

セキュリティ修正は最新リリースに対してのみ提供されます。

| Version | Supported |
| ------- | --------- |
| latest  | :white_check_mark: |
| < latest | :x:      |

## Verifying Downloads

v0.8.0 より後のリリースでは、GitHub Releases に置いているファイルに [GitHub の Artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations)（来歴の証明）を付けています。対象はインストーラ（`-setup.exe` と `.msi`）と、Store 用のファイル（`muSQL-store-x64.exe` と `muSQL-store-x64.msix`）です。ダウンロードしたファイルが、このリポジトリのリリース用ワークフローで作られたものかを、[GitHub CLI](https://cli.github.com/) で確かめられます。

```bash
gh attestation verify muSQL_X.Y.Z_x64-setup.exe --repo kan/musql \
  --signer-workflow kan/musql/.github/workflows/release.yml
```

Microsoft Store からインストールした muSQL は対象外です（Store が配るファイルは Microsoft が署名し直すため）。アプリ内の自動更新は、アプリに埋め込んだ公開鍵による署名の検証で別に守られています。

## Reporting a Vulnerability

脆弱性を発見した場合は、**公開 issue を作成せず**、以下のいずれかの方法で非公開に報告してください。

### GitHub Security Advisories（推奨）

[Report a vulnerability](https://github.com/kan/musql/security/advisories/new) からプライベートな脆弱性報告を作成してください。

### 報告に含めてほしい情報

- 脆弱性の種類（例: 認証情報の漏洩、SSH 接続の MITM、SQL インジェクション 等）
- 影響範囲（影響を受けるバージョン・機能・前提条件）
- 再現手順
- 想定される攻撃シナリオ・影響度
- 可能であれば修正案・PoC

### 対応方針

- **初動応答**: 報告受領から 7 日以内に確認連絡
- **修正リリース**: 重大度に応じて 30 日以内を目標
- **公開**: 修正リリース後に GitHub Security Advisory で詳細を公開（報告者のクレジット記載、希望があれば匿名）

## Scope

本プロジェクト（muSQL アプリ本体・付随する CI/CD・配布物）に関する脆弱性が対象です。

依存クレートの脆弱性については、まず upstream のセキュリティポリシーをご確認ください。muSQL での扱いは [Dependabot alerts](https://github.com/kan/musql/security/dependabot) で追跡しています。
