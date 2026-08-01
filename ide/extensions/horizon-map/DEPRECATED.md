# DEPRECATED — not the product surface

`ide/extensions/horizon-map` was an early scaffolding shortcut that used the
VS Code **extension API** (WebviewView + Extension Development Host).

That **violates** the locked product decision:

> Own/compile/ship a Code-OSS fork. Map lives as first-class workbench code
> under `src/vs/workbench/contrib/horizon/` (built-in EditorPane) — **not** an extension.

## What to use instead

| Role | Path |
|---|---|
| **Product Map** | [`ide/contrib/horizon/`](../../contrib/horizon/) |
| Synced into Code-OSS | `ide/code-oss/src/vs/workbench/contrib/horizon/` |
| Bootstrap / build / run | `./ide/scripts/bootstrap.sh` → `build.sh` → `run.sh` |

## What remains here

Media and UI logic under `media/` were **migrated** into
`ide/contrib/horizon/browser/media/`. This package may still hold historical
sidecar / inspection TypeScript for reference until W3′/W4 finish porting into
the workbench contrib.

Do **not**:

- Treat `./ide/scripts/dev-extension.sh` as the shipping path
- Publish this package to Open VSX / Marketplace
- Sync this folder into `code-oss/extensions/` as the product Map

See [`docs/requirements/ide-mvp-plan.md`](../../../docs/requirements/ide-mvp-plan.md).
