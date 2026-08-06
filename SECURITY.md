# Security Policy

## Supported Versions

Cofferly is early-stage software. Security fixes target the latest released version.

## Reporting a Vulnerability

Please do not open a public issue for vulnerabilities that could expose private family data or weaken parent-mode behavior.

Report security concerns privately through GitHub's private vulnerability reporting if enabled, or contact the maintainer directly through GitHub.

## Security Scope

Cofferly uses a generated six-object Coffer Story as both a family-use editing lock and the input for local data-file encryption. It should not be treated as absolute security.

Private ledger data is stored locally on the user's machine.

### Data at rest

Ledger data is encrypted at rest with XChaCha20-Poly1305 (authenticated encryption). The 32-byte key is derived from the canonical Coffer Story encoding using Argon2id (64 MiB memory, 3 iterations, 1 parallelism lane). The salt and nonce are random per file and stored alongside the ciphertext. Derived keys and plaintext serialization/decryption buffers are zeroized when dropped.

### Story guessing and cooldowns

Consecutive wrong unlock attempts receive escalating in-app cooldowns of 1, 2, 5, 15, 30, and then 60 minutes. The delay resets after a successful unlock and never becomes a permanent lockout. It is an interface-level deterrent: restarting the app resets it, and it cannot restrict an attacker testing a copied data file offline.

Six ordered, distinct objects selected from 30 yield 427,518,000 possibilities (about 28.7 bits). Argon2id makes each guess cost CPU time and 64 MiB of memory. This remains a family-use control, not a claim of absolute security: parallel or optimized offline attacks can be faster, and shoulder surfing is a practical risk.
