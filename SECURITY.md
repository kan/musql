# Security Policy

## Supported Versions

セキュリティ修正は最新リリースに対してのみ提供されます。

| Version | Supported |
| ------- | --------- |
| latest  | :white_check_mark: |
| < latest | :x:      |

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
