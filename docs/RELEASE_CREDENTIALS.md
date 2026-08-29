# Release credentials and configuration

Target repository: PORTALSURFER/chromascope

This document is generated from the same contract shown by the AudioDev
bootstrapper. The credentials stage can create or update only the GitHub
Actions entries listed here. It never handles PortalSurfer server-side
secrets, SSH keys, or deployment configuration.

## GitHub Actions secrets

Add these in GitHub at PORTALSURFER/chromascope -> Settings -> Secrets and
variables -> Actions. The Apple and PortalSurfer release entries belong to
the production environment.

| Name | Destination | Required when | Purpose |
| --- | --- | --- | --- |
| APPLE_DEVELOPER_ID_APPLICATION_CERT_BASE64 | production environment secret | Before the first release workflow run; not preflight | Base64 password-protected Developer ID Application .p12 containing the certificate and private key. |
| APPLE_DEVELOPER_ID_APPLICATION_CERT_PASSWORD | production environment secret | Before the first release workflow run; not preflight | Password for the .p12 import. |
| APPLE_NOTARY_KEY_BASE64 | production environment secret | Before the first release workflow run; not preflight | Base64 App Store Connect API private .p8 key for notarytool. |
| APPLE_NOTARY_KEY_ID | production environment secret | Before the first release workflow run; not preflight | App Store Connect API key ID. |
| APPLE_NOTARY_ISSUER_ID | production environment secret | Before the first release workflow run; not preflight | App Store Connect issuer ID. No separate team-ID field is read. |
| APPLE_CODESIGN_IDENTITY | production environment secret | Optional, only if automatic identity selection is ambiguous | Explicit Developer ID Application identity override. |
| PORTALSURFER_RELEASE_TOKEN | production environment secret | Before a published release (publish=true); not preflight/package-only | PortalSurfer release-publisher bearer credential; it has no GitHub API scope. |
The generated workflows reference no GitHub Actions variables, so the
credentials stage creates no variables. GitHub's automatic GITHUB_TOKEN is
used for the metadata-only GitHub Release operation.

## Credential-stage setup

Preview without mutation:

    cargo run --manifest-path audiodev-plugin-bootstrap/Cargo.toml -- credentials \
      --plugin /path/to/chromascope

Execute only from an interactive terminal after reviewing the plan:

    cargo run --manifest-path audiodev-plugin-bootstrap/Cargo.toml -- credentials \
      --plugin /path/to/chromascope --execute

The stage first checks gh auth status without --show-token, verifies
repository ADMIN access, checks the repository and production-environment
Actions public keys, and inventories names only. It then shows the checkpoint
and requires the exact confirmation SET CREDENTIALS chromascope.

Secret values are entered with terminal echo disabled and passed only through
gh secret set standard input; the --body option is omitted so gh reads that
input. They are never accepted as
arguments, files, environment variables, plan output, generated config, or
logs, and are dropped after each update. A blank prompt retains an existing
entry; required missing entries must be supplied. Non-interactive execute mode
is rejected. The CLI never reads local Apple key files, server credentials, or
SSH keys.

PORTALSURFER_RELEASE_TOKEN is not a GitHub API token. The current PortalSurfer
server accepts the generic release-upload bearer credential and compares it
with PORTALSURFER_RELEASE_UPLOAD_TOKEN_SHA256 (preferred) or the raw
fallback. This repository does not define a token-issuance command; obtain and
provision that value through the PortalSurfer operator's approved process,
store the matching hash on the server, and enter the bearer value only in the
hidden GitHub environment-secret prompt.

## PortalSurfer server and deployment configuration

These values are deliberately outside the credentials stage and must be
configured by the PortalSurfer operator:

| Name | Configuration location | Notes |
| --- | --- | --- |
| PORTALSURFER_RELEASE_UPLOAD_TOKEN_SHA256 | PortalSurfer server .env | Preferred SHA-256 hash of the matching release bearer token. |
| PORTALSURFER_RELEASE_UPLOAD_TOKEN | PortalSurfer server .env | Raw fallback only when the hash is not configured. |
| AUDIODEV_PRODUCTS_FILE | PortalSurfer compose environment / mounted config | Normally /config/audiodev-products.json. |
| PORTALSURFER_DEPLOY_SERVER, PORTALSURFER_DEPLOY_USER | scripts/deploy.sh environment or flags | Only when defaults are not correct. |
| PORTALSURFER_DEPLOY_KEY_PATH | Local scripts/deploy.sh setting | Optional SSH key path; the bootstrapper never reads or uploads it. |
| PORTALSURFER_REMOTE_PATH, PORTALSURFER_SITE_DOMAIN, PORTALSURFER_PUBLIC_ORIGIN | scripts/deploy.sh environment or flags | Only when deployment defaults need overriding. |
| PORTALSURFER_ANCHOR_PATH | scripts/deploy.sh environment | Only when Anchor is not at the checkout's sibling ../anchor. |

The release mechanism is GitHub Actions; local CLI commands do not publish
releases. Deployment remains a separate final stage requiring its own
DEPLOY chromascope confirmation.
