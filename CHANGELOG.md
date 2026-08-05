# Changelog

## Unreleased

- Renames the encrypted data file from `data.json` to `vault.cofferly`. Existing current-format files are copied atomically, verified byte-for-byte, and retained untouched as a recovery backup.
- **Security:** Removes plaintext and pre-Cofferly import paths. Unsupported data files now remain locked and are never overwritten automatically.
- **Performance / crypto:** Envelope encryption derives Argon2id once per unlock and caches a data key for the session, so saves do not freeze the UI. PIN unlock runs Argon2id off the UI thread, and successful unlocks no longer rewrite an unchanged data file.
- Printable ledgers are written to the OS temp directory and cleaned up on launch, instead of persisting plaintext HTML next to encrypted data.
- Sidebar wallet cards expose name and balance to screen readers via AccessKit labels.
- Parent mode auto-locks after 10 minutes of inactivity.
- Keyboard ergonomics: Enter submits the entry form, Esc closes Settings, PIN auto-submits on the 4th digit.
- Window geometry plus selected wallet and ledger sort order are restored between launches (unlock state is never persisted).
- Status area distinguishes Info / Success / Error (color plus a non-color error prefix).
- Ledger table caches sorted rows and uses virtualized row layout.
- Add encryption for the local data file using Argon2id key derivation + XChaCha20-Poly1305 authenticated encryption. Data is encrypted at rest with the parent PIN to prevent casual tampering.
- Adds a Settings window for wallet management, parent PIN updates, starting balance edits, wallet deletion confirmation, and remove-entry undo.
- Splits rendering code into a dedicated views module and zeroizes derived keys plus plaintext serialization/decryption buffers when they are dropped.
- Refreshes README screenshots and updates documentation for the current Cofferly UI and security model.
- Renames the app to Cofferly after product-name collisions with earlier names.
- Updates all references in code, docs, build scripts, installer, tests, screenshots, and GitHub workflows.

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
