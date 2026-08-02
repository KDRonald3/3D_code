# Horizon IDE branding assets

Placeholder directory for product icons and splash art used when packaging
Horizon IDE from Code-OSS.

## Expected files (not yet authored)

| File | Use |
|---|---|
| `icon.png` | Generic app icon (1024×1024 preferred) |
| `code.png` / `code.svg` | Linux desktop icon source (`linuxIconName`: `horizon-ide`) |
| `code.ico` | Windows installer / exe icon |
| `code.icns` | macOS bundle icon |
| `letterpress*.svg` | Empty-window letterpress (optional) |

Until custom artwork lands, Code-OSS keeps its stock icons after bootstrap.
Copy branded assets here and wire them from `bootstrap.sh` / packaging scripts
when ready (for example into `resources/` under `ide/code-oss/`).

Do not commit binary drafts larger than needed for the MVP shell.
