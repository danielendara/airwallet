# GitHub repository settings

Cofferly is **public**. You remain the only admin. Production releases stay on GitHub Releases (tag `v*`).

## Who can do what

| Actor | Push `main` | Merge PRs | Change settings | Publish a release |
|-------|-------------|-----------|-----------------|-------------------|
| Maintainer (`danielendara`) | Via PR + CI (admin bypass only for emergencies) | Yes | Yes | Yes (`v*` tag / workflow_dispatch) |
| Outside PR / fork | No | No | No | No |
| Dependabot | Opens PRs only | No | No | No |

## Applied on GitHub

- **Default branch:** `main`
- **Merges:** squash only; delete head branch on merge
- **Homepage:** [latest release](https://github.com/danielendara/cofferly/releases/latest)
- **Wiki / Projects:** off
- **Actions:** default `contents: read`; workflows cannot approve PRs
- **CODEOWNERS:** `*` → `@danielendara`
- **Secret scanning** + **push protection** + **Dependabot security updates**
- **Private vulnerability reporting:** on
- **Ruleset `protect-main`:** [rules/20830863](https://github.com/danielendara/cofferly/rules/20830863)
  - Pull request required (0 extra approvals — solo maintainer)
  - Conversation resolution required
  - Required checks (strict / up to date):
    - `Test (ubuntu-latest)`
    - `Test (windows-latest)`
    - `Test (macos-latest)`
    - `Security audit`
  - No force-push, no deleting `main`
  - Bypass actor: you (emergency only)

Classic branch protection was replaced by this ruleset so required checks actually match the CI job names.

## Collaborators

Do not add write access casually. Prefer PRs from forks. Direct write is only for people you trust with releases and family-data handling.
