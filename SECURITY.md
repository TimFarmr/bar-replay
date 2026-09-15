# Security

## What this app does with your data

- **Market data** goes directly from the provider to your machine. There is no
  server operated by this project, so there is nothing of ours in the path.
- **API keys** are stored in your operating system's keychain — Windows
  Credential Manager, macOS Keychain, or the Linux secret service. They are
  never written to a config file, never written to a log, and never sent
  anywhere except that provider's own API.
- **Your sessions, trades and journal** are files on your disk. Nothing is
  uploaded, and there is no telemetry of any kind.

## Reporting a vulnerability

Please report privately through
[GitHub's security advisories](https://github.com/TimFarmr/bar-replay/security/advisories/new)
rather than opening a public issue.

Things worth reporting: anything that could leak an API key, anything that
reaches the network other than a provider endpoint the user chose, or any path
that lets a crafted CSV or provider response execute code.

Because this is a small volunteer project, the honest expectation is a reply
within a couple of weeks rather than a guaranteed window.

## A note on unsigned installers

Releases are not code-signed, so your OS will warn on first launch. That warning
is doing its job: it cannot tell a legitimate unsigned build from a tampered
one. If that matters to you, [build from source](README.md#building-from-source)
instead — the whole toolchain is reproducible from this repository.
