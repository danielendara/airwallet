# Release Checklist

Use this checklist when preparing a Cofferly release.

## Local Checks

```powershell
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
.\scripts\package-windows.ps1 -Version 0.3.0
```

The portable zip will be created in `dist/`.

## README screenshots

Refresh the three README images after meaningful UI changes (requires a graphical desktop):

```bash
cargo build --release
./scripts/capture-screenshots.sh
```

This runs Cofferly with `COFFERLY_CAPTURE` / `COFFERLY_DATA_DIR` (isolated vault) and overwrites:

- `docs/screenshots/cofferly-story-unlock.png`
- `docs/screenshots/cofferly-wallet-screen.png`
- `docs/screenshots/cofferly-settings-screen.png`


## Windows Installer

The repository includes `installer/Cofferly.iss` for Inno Setup.

### CI (preferred)

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds the portable zip, compiles the installer with Inno Setup, and attaches both to the GitHub Release.

### Local

1. Install Inno Setup 6.
2. Build Cofferly with `cargo build --release`.
3. Compile with a version override if needed:

```powershell
& "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" /DMyAppVersion=0.3.0 installer\Cofferly.iss
```

Output is written to `dist/` (`Cofferly-{version}-Setup.exe`).

## GitHub Release

1. Update `Cargo.toml`, `README.md`, installer default version, and this checklist if the version changes.
2. Refresh the README screenshots in `docs/screenshots/` if the UI changed.
3. Commit the release and merge to `main`.
4. Tag it, for example `v0.3.0`, and push the tag.
5. Confirm the Release workflow attached `Cofferly-*-windows-x64.zip` and `Cofferly-*-Setup.exe`.

## Repository Controls

Before announcing a release, review [GITHUB_SETTINGS.md](GITHUB_SETTINGS.md).

## Manual Smoke Test

Before publishing, open Cofferly and verify:

- The app launches as `Cofferly`.
- Coffer Story setup appears first, generates six objects, and requires confirmation from a shuffled grid.
- Restart after setup and unlock with the same Coffer Story.
- A legacy v2 PIN file opens the Legacy PIN screen and migrates only after Coffer Story confirmation.
- Both default child wallets are visible.
- Settings opens from the top bar.
- The selected wallet can be renamed.
- The selected wallet starting balance can be changed.
- A new child wallet can be added.
- Wallet deletion asks for confirmation and keeps at least one wallet.
- Adding a deposit changes the running balance.
- Adding a deduction changes the running balance.
- Remove latest entry offers undo.
- Changing the Coffer Story saves and unlocks with the new story.
- A current encrypted `data.json` is copied to `vault.cofferly`, verifies successfully, and remains in place as a backup.
- When both data files exist, `vault.cofferly` takes precedence and neither file is modified during startup.
- Print this wallet opens a printable browser page.
- Print all wallets opens every wallet in one printable page.
- Export this wallet CSV and Export all wallets CSV open a local spreadsheet file (no `$` in amounts).
- The date field defaults to today; a past date is accepted; a future date is rejected.
- Remove latest entry removes the newest-dated row when the ledger is backdated.
- Settings shows the Cofferly version next to the save reminder.
- Starting-balance Save stays disabled until the opening amount actually changes.
- Parent mode shows a quiet “Locks in …” countdown in the last two minutes of inactivity.
- The first two wrong Coffer Story (or PIN) attempts do not start a cooldown; a third does.
- Undo last pick and Cancel/Back work during story setup and story change.
- Already-picked story objects are greyed out and ignored.
- Print recovery card opens a local HTML card during story setup.
- With three or more wallets, the selected wallet is the same after restart.
