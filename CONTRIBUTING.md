# Contributing

Cofferly is a maintainer-led family app. The repository is public so people can learn from it, audit it, fork it, and propose useful improvements, but it is not open for direct unreviewed changes.

## Contribution Policy

- Open an issue before starting larger work.
- Keep pull requests focused and small.
- Do not include real children's names, family data, screenshots with private data, or personal ledger entries.
- Do not add telemetry, cloud sync, ads, accounts, or network features without prior maintainer approval.
- Do not change licensing, release automation, or security-sensitive behavior without prior discussion.
- All pull requests require maintainer review before merging.

## Local Development

```powershell
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo audit
cargo build --release
```

If `cargo` is not on PATH on Windows, add `%USERPROFILE%\.cargo\bin` to PATH or run Cargo from that folder.

CI runs the same checks on Linux, macOS, and Windows (plus `cargo audit` on Ubuntu).

## Pull Request Checklist

- Formatting and clippy pass.
- Tests pass.
- The app still opens to the Coffer Story (or Legacy PIN migration) unlock screen.
- Public files do not contain real child names or private family data.
- README or release docs are updated when behavior changes.

## Maintainer Control

Only maintainers decide whether a contribution fits Cofferly. A public repository does not mean every proposed change will be accepted.

