# Horizon workbench contribution

**This is the product surface** for the Horizon Map inside Horizon IDE (a branded
Code-OSS fork). It is **not** a VS Code / Open VSX extension.

## Role

| Piece | Path |
|---|---|
| Source of truth (this repo) | `ide/contrib/horizon/` |
| Synced into Code-OSS | `ide/code-oss/src/vs/workbench/contrib/horizon/` |
| Registration | imported from `workbench.common.main.ts` via bootstrap/sync |

The Map opens as a built-in **EditorPane** (`HorizonMapEditorPane`) with a
singleton `HorizonMapInput` (`horizon-map:` scheme). Commands:

| Command ID | Action |
|---|---|
| `horizon.map.open` / `horizon.map.show` | Open / focus the Map EditorPane |
| `horizon.map.toggle` | Map ↔ classic editors |
| `horizon.map.hide` | Close map, focus classic |
| `horizon.map.analyse` | Open map + request workspace analyse |

## Layout

```text
ide/contrib/horizon/
  README.md                 ← you are here
  common/horizon.ts         ← IDs, protocol types
  browser/
    horizon.contribution.ts ← register EditorPane, commands, menus
    horizonEditorPane.ts    ← EditorPane + webview host
    horizonInput.ts         ← EditorInput
    horizonHtml.ts          ← media HTML / CSP / asWebviewUri
    media/                  ← map viewer assets (from deprecated extension)
```

## Sync / build / run

```bash
./ide/scripts/bootstrap.sh          # clone Code-OSS, brand, sync contrib
./ide/scripts/sync-contrib.sh       # re-copy contrib + wire import
./ide/scripts/build.sh              # npm run compile inside code-oss
./ide/scripts/run.sh [workspace]    # launch Horizon IDE (forked product)
```

`ide/extensions/horizon-map/` is **deprecated as product** — see its
`DEPRECATED.md`. Media was migrated here; do not ship the extension API package.

## Workstream status

| Area | Status |
|---|---|
| EditorPane + commands + media webview | This folder (W2′) |
| Read-only inspection + rust-analyzer | W3′ |
| Sidecar spawn/attach from contrib | W4 |
| IDE UI smoke tests | W5 |

See [`docs/requirements/ide-mvp-plan.md`](../../../docs/requirements/ide-mvp-plan.md).
