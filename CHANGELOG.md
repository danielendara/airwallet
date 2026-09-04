# Changelog

## Unreleased

- Amount and starting-balance validation copy mentions grouped/$ examples (`$1,234.56`) the parser already accepts.
- Widens the main status chip so long export/unlock messages stay readable.
- Entry-form validation failures move keyboard focus to the first invalid field.
- Amount and starting-balance fields accept pasted grouped money (`$1,234.56`) by ignoring thousands separators.
- Renaming a wallet keeps the Settings name field filled so it doesn't look like the name was lost.
- Already-picked Coffer Story objects are ignored and greyed out so a duplicate tap cannot count as a failed unlock.
- Settings starting-balance field prefills and hints the wallet's opening balance, not the running total, and Save stays disabled until the value actually changes.
- Adding a wallet from Settings resets the starting-balance field to the new wallet's opening amount so Save cannot write the previous kid's prefill.
- Deleting a wallet from Settings resets the name and starting-balance fields to the remaining wallet so Save cannot overwrite it.
- CSV export for one wallet or all wallets (temp file, local open, no cloud).
- Settings shows the Cofferly version next to the save reminder.
- Money amounts use thousands separators (`$1,234.56`); typed input is unchanged.
- Transactions can be backdated (MM/DD/YYYY, default today; future dates are rejected).
- Parent mode shows a quiet “Locks in …” countdown for the last two minutes of inactivity.
- README: macOS/Linux `cargo run` / `cargo build --release` notes.
- README: primary Download links to GitHub Releases; recovery-card guidance and PIN→Story upgrade notes.
- Adds printable [docs/recovery-card.md](docs/recovery-card.md) template for Coffer Stories.
- CI: multi-OS test/clippy/fmt workflow plus `cargo audit` on PRs and `main`.
- Release workflow: compile Inno Setup installer, attach zip + Setup.exe to the GitHub Release.
- deps: bump `eframe` / `egui_extras` to 0.36.
- Refreshes README screenshots for Coffer Story unlock, sample ledger, and Settings (including Change Coffer Story); removes obsolete PIN-screen asset.
- Adds maintainer screenshot helper (`scripts/capture-screenshots.sh` + `COFFERLY_CAPTURE` env).

## 0.2.0 — 2026-08-06

First feature release after the initial public cut: stronger parent unlock, vault file naming, and a pile of security/UX hardening.

### Security

- Replaces new-install parent PINs with generated six-object **Coffer Stories**, encrypted-envelope v3, shuffled object grids, confirmation, and legacy PIN migration.
- Escalating **1-to-60-minute UI cooldowns** after consecutive wrong unlock attempts (no permanent lockout).
- Removes plaintext and pre-Cofferly import paths. Unsupported data files stay locked and are never overwritten automatically.
- Envelope encryption: Argon2id once per unlock with a session data-key cache so saves stay responsive; unlock work runs off the UI thread.
- Local data at rest: Argon2id + XChaCha20-Poly1305; derived keys and plaintext buffers are zeroized on drop.

### Data file

- Renames the encrypted data file from `data.json` to **`vault.cofferly`**. Existing current-format files are copied atomically, verified byte-for-byte, and kept as an untouched recovery backup.

### UX and accessibility

- Settings window for wallet management, Coffer Story / PIN updates, starting balance, wallet deletion confirmation, and remove-entry undo.
- Parent mode auto-locks after 10 minutes of inactivity.
- Keyboard ergonomics: Enter submits the entry form, Esc closes Settings.
- Window geometry, selected wallet, and ledger sort order restore between launches (unlock state is never persisted).
- Status area distinguishes Info / Success / Error (color plus a non-color error prefix).
- Sidebar wallet cards expose name and balance to screen readers via AccessKit labels.
- Ledger table caches sorted rows and uses virtualized row layout.
- Printable ledgers go to the OS temp directory and are cleaned up on launch (no plaintext HTML next to the vault).
- Refreshed README screenshots and docs for the current UI and security model.
- App branding, installer, and packaging aligned under the **Cofferly** name.

### Dependencies

- Cargo lockfile refresh (including `quick-xml` advisory fix via transitive Wayland path).

## 0.1.0

- Initial desktop app, later renamed Cofferly.
- Adds two default child wallets.
- Tracks deposits, deductions, and running balances.
- Adds parent PIN unlock with first-run PIN `1234`.
- Adds child wallet renaming and adding custom child wallets.
- Adds printable ledger export for one wallet or both wallets.
- Adds Windows portable packaging script.
- Adds Inno Setup installer script.
- Adds GitHub Actions release workflow.
- Updates GitHub Actions and Rust dependencies after first Dependabot scan.
