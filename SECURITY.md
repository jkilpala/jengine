# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for a security problem. Report it
privately through GitHub's
[security advisory form](https://github.com/jkilpala/jengine/security/advisories/new).
Include what is affected, how to reproduce it, and what an attacker could do.

You will get an answer within a week. Once a fix is ready it is released and
the advisory published, crediting you unless you prefer otherwise.

## Scope

The parts where security matters most are the network-facing ones:

- `crates/jengine-server` - the game server framework, its admin API and
  dashboard, accounts, clusters and the `jengine-fleet` deployment agent
- `crates/jengine-relay`
- live ops snapshot signing (`crates/jengine-liveops`) and asset pack
  verification

A game built with the engine runs its own scripts and assets with full trust,
so "a malicious scene or script can do X" is not a vulnerability in itself.

## Supported versions

Before 1.0 only the latest release receives security fixes.

## Known limits

The server framework's known limits, several of them security-relevant, are
listed openly in [crates/jengine-server/LIMITS.md](crates/jengine-server/LIMITS.md).
