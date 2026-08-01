# Release Checklist

Use this checklist when preparing a Cofferly release.

## Local Checks

```powershell
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
.\scripts\package-windows.ps1 -Version 0.1.0
```

The portable zip will be created in `dist/`.

## Windows Installer

The repository includes `installer/Cofferly.iss` for Inno Setup.

1. Install Inno Setup.
2. Build Cofferly with `cargo build --release`.
3. Open `installer/Cofferly.iss`.
4. Compile the installer.

The installer output is written to `dist/`.

## GitHub Release

1. Update `Cargo.toml`, `README.md`, and this checklist if the version changes.
2. Refresh the README screenshots in `docs/screenshots/` if the UI changed.
3. Commit the release.
4. Tag it, for example `v0.1.0`.
5. Push the tag to GitHub.
6. Attach the generated portable zip and installer to the GitHub Release.

## Repository Controls

Before announcing a release, review [GITHUB_SETTINGS.md](GITHUB_SETTINGS.md).

## Manual Smoke Test

Before publishing, open Cofferly and verify:

- The app launches as `Cofferly`.
- The parent PIN screen appears first.
- PIN `1234` unlocks a fresh install.
- Both default child wallets are visible.
- Settings opens from the top bar.
- The selected wallet can be renamed.
- The selected wallet starting balance can be changed.
- A new child wallet can be added.
- Wallet deletion asks for confirmation and keeps at least one wallet.
- Adding a deposit changes the running balance.
- Adding a deduction changes the running balance.
- Remove latest entry offers undo.
- Changing the parent PIN saves and unlocks with the new PIN.
- Print this wallet opens a printable browser page.
- Print all wallets opens every wallet in one printable page.
