# Changelog

## Unreleased

## 0.3.0 — 2026-09-09

CSV export, backdated entries, grouped money, and Coffer Story / Settings hardening after the 0.2.0 cut.

### Features

- CSV export for one wallet or all wallets (temp file, local open, no cloud). Amounts are unformatted decimals so spreadsheets can sum them.
- Money amounts use thousands separators (`$1,234.56`); typed input is unchanged. Pasted grouped money (`$1,234.56`) is accepted by ignoring thousands separators.
- Amount and starting-balance validation copy mentions grouped/$ examples the parser already accepts.
- Transactions can be backdated (MM/DD/YYYY, default today; future dates are rejected).
- Parent mode shows a quiet “Locks in …” countdown for the last two minutes of inactivity.
- Settings shows the Cofferly version next to the save reminder.
- Adds printable [docs/recovery-card.md](docs/recovery-card.md) template for Coffer Stories, plus an in-app **Print recovery card** action during story setup.

### Coffer Story

- First two wrong unlock attempts are free (grace), then the existing 1-to-60-minute cooldown ladder starts.
- Undo-last-pick and Cancel/Back on Coffer Story setup, change, and PIN migration.
- Already-picked objects are ignored and greyed out so a duplicate tap cannot count as a failed unlock.
- Unlock grid is exposed to screen readers.
- Story setup crypto (Argon2id) runs on a background thread so the window stays responsive.

### Security

- CSV text fields that start with `=`, `+`, `-`, `@`, tab, or CR are prefixed with `'` so spreadsheets treat them as text, not formulas.
- Recovery-card and ledger export temp files use private, random names and are deleted on lock and exit (with a launch-time sweep as backup).

### UX and accessibility

- Widens the main status chip and unlocked chrome wallet picker/sidebar to 300px so long export/unlock messages stay readable.
- Tightens left-panel vertical density for 1280×800.
- Entry-form validation failures move keyboard focus to the first invalid field.
- Renaming a wallet keeps the Settings name field filled.
- Settings starting-balance field prefills and hints the wallet's opening balance, not the running total, and Save stays disabled until the value actually changes.
- Adding or deleting a wallet from Settings resets name/starting-balance fields so Save cannot write the previous kid's prefill.
- Remove latest entry deletes the date-newest row (not last-appended), matching backdated ledgers.
- Switching wallets from the sidebar clears a pending undo from the previous wallet.
- Selected wallet is preserved across restart for families with more than two wallets.

### CI and packaging

- Multi-OS test/clippy/fmt workflow plus `cargo audit` on PRs and `main`.
- Coverage report and 74% line floor (excluding UI-only `views.rs` and screenshot `capture.rs`).
- Release workflow compiles the Inno Setup installer and attaches zip + Setup.exe to the GitHub Release.
- README: macOS/Linux `cargo run` / `cargo build --release` notes; primary Download links to GitHub Releases; recovery-card guidance and PIN→Story upgrade notes.
- Refreshes README screenshots for Coffer Story unlock, sample ledger, and Settings; removes the obsolete PIN-screen asset.
- Adds maintainer screenshot helper (`scripts/capture-screenshots.sh` + `COFFERLY_CAPTURE` env).

### Dependencies

- Bump `eframe` / `egui_extras` to 0.36.
- Bump `argon2` to 0.6 (Argon2id parameters unchanged: 64 MiB, 3 iterations, 1 lane).

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
