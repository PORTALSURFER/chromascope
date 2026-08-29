# Release credentials and configuration

Target repository: PORTALSURFER/chromascope

The staged bootstrapper deliberately does not ask for, read, display, or write secret values. Add these names directly in the listed configuration locations:

| Name | Location |
| --- | --- |
| APPLE_DEVELOPER_ID_APPLICATION_CERT_BASE64 | GitHub production environment |
| APPLE_DEVELOPER_ID_APPLICATION_CERT_PASSWORD | GitHub production environment |
| APPLE_NOTARY_KEY_BASE64 | GitHub production environment |
| APPLE_NOTARY_KEY_ID | GitHub production environment |
| APPLE_NOTARY_ISSUER_ID | GitHub production environment |
| PORTALSURFER_RELEASE_TOKEN | GitHub production environment when publishing |
| RADIANT_REPO_TOKEN | GitHub repository secrets |
| PORTALSURFER_RELEASE_UPLOAD_TOKEN_SHA256 | PortalSurfer server .env (preferred hash) |
| PORTALSURFER_RELEASE_UPLOAD_TOKEN | PortalSurfer server .env (raw fallback) |
| AUDIODEV_PRODUCTS_FILE | PortalSurfer compose environment; normally /config/audiodev-products.json |
| PORTALSURFER_DEPLOY_SERVER / USER / KEY_PATH / REMOTE_PATH / SITE_DOMAIN / PUBLIC_ORIGIN | Existing scripts/deploy.sh environment or flags |
| PORTALSURFER_ANCHOR_PATH | Existing scripts/deploy.sh environment when Anchor is outside the site checkout's ../anchor path |

PORTALSURFER_RELEASE_TOKEN is the GitHub publisher credential; the PortalSurfer server upload credential is a separate value. Keep both outside this repository and configure them through the approved GitHub/server secret mechanisms.
