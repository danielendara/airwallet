# Changelog

## Unreleased

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
