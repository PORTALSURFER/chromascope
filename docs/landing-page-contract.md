# Landing-page and release integration contract

The bootstrap flow keeps product release metadata and rendered page content separate. site/product.json is the release/catalog input; site/landing-page.json is the complete page-template content consumed by the PortalSurfer generator.

## Stages

1. init creates the independent local Toybox repository and both site inputs.
2. remote optionally creates/configures PORTALSURFER/chromascope and pushes only with --execute. The generated release workflows publish to /plugins/api/v1/products/chromascope/releases.
3. landing renders /plugins/chromascope/, updates the catalog, and idempotently upserts hosting/audiodev-products.json; it never handles credentials.
4. deploy invokes PortalSurfer's scripts/deploy.sh only with --execute, after showing the target and requiring DEPLOY chromascope. It then checks the public page title with curl.

## Product release contract

- Stable slug: chromascope
- Repository: PORTALSURFER/chromascope
- Release API: `/plugins/api/v1/products/chromascope/releases`
- Page: `/plugins/chromascope/`
- Formats: VST3
- Stable/RC platform: macOS arm64
- Nightly platforms: macOS arm64 signed plus Windows x86_64 explicitly unsigned
- Stable/RC release manifest: schema 2
- Nightly release manifest: one PortalSurfer schema 3 release containing both
  platform archives; screenshot and changelog remain release metadata, not
  download artifacts

## Safety

Plan mode is the default for every stage and for bootstrap. Input is validated before an execute stage mutates anything. Existing generated output is resumed only when its identity matches; unrelated landing pages are never overwritten. Secret values, SSH keys, GitHub tokens, and server .env values remain outside this CLI. The nightly release job uses the existing production environment and a short-lived GitHub OIDC attestation; it does not add an OIDC secret.
