# Privacy Policy

Last updated: 2026-03-27

## Overview

muSQL is a desktop MySQL client for Windows. This application is designed to run locally on your device, and we do not operate any servers that collect your data.

## Data Stored Locally

The following data is stored exclusively on your device and is never transmitted to us:

- **Connection profiles** (host, port, username, SSH settings) — stored in the application's local data directory
- **Passwords and API keys** — stored in Windows Credential Manager (OS keyring), never in plain text files
- **Application preferences** (theme, language, window state) — stored in localStorage
- **SQL query history and drafts** — stored in localStorage
- **AI chat history** — stored in localStorage per database

## Network Communication

muSQL communicates over the network only in the following cases:

### Database and SSH Connections
muSQL connects directly to MySQL servers and SSH servers that you explicitly configure. Connection credentials are sent only to the servers you specify.

### AI Assist (Optional)
When you use the AI assist feature, your database schema and natural language prompts are sent to the AI provider you select (Anthropic Claude, OpenAI, or Google Gemini). This feature is opt-in and requires you to provide your own API key. Please refer to each provider's privacy policy for details on how they handle your data.

### Update Check
muSQL periodically checks GitHub for available updates. No personal data is transmitted during this check.

### Docker Integration
When you use the Docker integration, muSQL communicates with the Docker daemon running on your local machine. No data is sent to external servers.

## Data We Collect

We do not collect any personal data, usage analytics, or telemetry.

## Third-Party Services

- **AI providers** (optional, user-initiated): [Anthropic](https://www.anthropic.com/privacy), [OpenAI](https://openai.com/privacy), [Google](https://policies.google.com/privacy)
- **GitHub** (update check): [GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)

## Contact

If you have questions about this privacy policy, please open an issue at https://github.com/kan/musql/issues.
