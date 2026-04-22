# Stardive Platform

Monorepo for:
- `stardive-api`: modular API server intended for `api.stardive.space`
- `stardive`: companion CLI (including egui file UI)
- `stardive-core`: shared contracts, config, and client helpers

## Quick start

```bash
cargo run -p stardive-api
cargo run -p stardive -- api health
```

## API endpoints (`/v1`)

- `GET /health`
- `POST /search/text`
- `POST /search/news`
- `POST /extract`
- `POST /files` (multipart field: `file`, max default `1_073_741_824` bytes)
- `GET /files`
- `GET /files/{id}`
- `POST /render/snippet` (`code`, optional `language`/`theme`, `format: svg|png`)
- `GET /installers`
- `GET /installers/{name}`
- `GET /eternal`
- `GET /eternal/{name}`

## Configuration

`stardive-api` environment variables:
- `STARDIVE_BIND_ADDR` (default `0.0.0.0:8080`)
- `STARDIVE_DATA_DIR` (default `data`)
- `STARDIVE_INSTALLERS_DIR` (default `installers`)
- `STARDIVE_ETERNAL_DIR` (default `eternal`)
- `STARDIVE_API_KEY` (optional; when set, bearer auth is enforced except `/v1/health`)
- `STARDIVE_MAX_UPLOAD_BYTES` (default `1073741824`)
- `STARDIVE_MAX_SNIPPET_CHARS` (default `20000`)
- `STARDIVE_ENABLE_HEALTH|SEARCH|FILES|RENDER|INSTALLERS|ETERNAL` (default `true`)

`stardive` CLI config:
- file: `~/.config/stardive/config.toml`
- keys: `base_url`, `api_key`
- precedence: CLI flags > env (`STARDIVE_BASE_URL`, `STARDIVE_API_KEY`) > config file > defaults

## CLI commands

- `stardive api health`
- `stardive search text --query \"...\"`
- `stardive search news --query \"...\"`
- `stardive extract --url \"https://...\"`
- `stardive file upload <path>`
- `stardive file list`
- `stardive file download <id> --output <path>`
- `stardive file gui`
- `stardive render snippet --code \"...\" --format svg --output out.svg`

## Static content

- `installers/`: shell scripts served via `/v1/installers`
- `eternal/`: static long-lived resources served via `/v1/eternal`
