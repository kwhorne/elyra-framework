# Security Policy

## Supported versions

Elyra is pre-1.0. Security fixes are applied to the latest `0.x` release.

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |
| < 0.1   | ❌        |

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Report privately via one of:

- GitHub's [private vulnerability reporting](https://github.com/kwhorne/elyra-framework/security/advisories/new)
  (Security → Report a vulnerability), or
- email **security@kwhorne.com**.

Please include:

- a description of the issue and its impact,
- steps to reproduce (a minimal proof of concept if possible),
- affected version(s) and platform.

You can expect an acknowledgement within a few days. We will work with you on a
fix and coordinate a disclosure timeline, and credit you in the release notes
unless you prefer to remain anonymous.

## Scope notes

The updater verifies release artifacts with an **ed25519 signature** before
applying them, so a compromised release server alone cannot push a malicious
binary. Self-update replaces the running executable; distributing signed builds
still requires re-signing (Developer ID + notarization on macOS). See
[docs/updater.md](docs/updater.md).
