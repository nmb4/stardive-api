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

## Install the `stardive` CLI

Recommended one-liner (downloads this repo script and runs it):

```bash
curl -fsSL https://raw.githubusercontent.com/nmb4/stardive-api/main/installers/install-stardive.sh | bash
```

PowerShell (Windows):

```powershell
irm https://raw.githubusercontent.com/nmb4/stardive-api/main/installers/install-stardive.ps1 | iex
```

What this does:
- uses the pkgx cURL bootstrap to create a temporary Rust toolchain environment
- runs `cargo install stardive` in that temporary environment
- installs only the final `stardive` binary to `/usr/local/bin/stardive` (uses `sudo` if needed)
- does not require permanently installing pkgx or rustup on your system

Direct crates.io install (if you already have Rust):

```bash
cargo install stardive
```

Local fallback from this repository:

```bash
cargo install --path crates/stardive
```

Run local without installing:

```bash
cargo run -p stardive -- --help
```

## API endpoints

- `GET /up` (top-level health endpoint for ONCE/Kamal-style checks)
- `GET /health`
- `POST /search/text`
- `POST /search/news`
- `POST /extract`
- `POST /files` (multipart field: `file`, max default `1_073_741_824` bytes)
- `GET /files`
- `GET /files/{id}`
- `POST /render/snippet` (`code`, optional `language`/`theme`, `format: svg|png`)
- `GET /lostandfound/health`
- `POST /lostandfound/auth/login`
- `GET /lostandfound/items`
- `GET /lostandfound/items/{id}`
- `POST /lostandfound/items`
- `PATCH /lostandfound/items/{id}/status`
- `GET /lostandfound/claims`
- `POST /lostandfound/claims`
- `GET /lostandfound/categories`
- `GET /installers`
- `GET /installers/{name}`
- `GET /eternal`
- `GET /eternal/{name}`

`lostandfound` is mounted under `/v1` like every module, so the full URL for login is `/v1/lostandfound/auth/login`.

## Configuration

`stardive-api` environment variables:
- `STARDIVE_BIND_ADDR` (default `0.0.0.0:8080`)
- `STARDIVE_DATA_DIR` (default `data`)
- `STARDIVE_LOG_DIR` (default `<STARDIVE_DATA_DIR>/logs`, daily-rotated debug logs)
- `STARDIVE_INSTALLERS_DIR` (default `installers`)
- `STARDIVE_ETERNAL_DIR` (default `eternal`)
- `STARDIVE_API_KEY` (optional; when set, bearer auth is enforced except `/v1/health`)
- `STARDIVE_MAX_UPLOAD_BYTES` (default `1073741824`)
- `STARDIVE_MAX_SNIPPET_CHARS` (default `20000`)
- `STARDIVE_ENABLE_HEALTH|SEARCH|FILES|RENDER|LOSTANDFOUND|INSTALLERS|ETERNAL` (default `true`)

Logging behavior:
- request and response logs are emitted at `info` level
- startup prints module availability and active config values at `info` level
- `debug`+ logs are also written to a daily-rotated file: `stardive-api.debug.log` in `STARDIVE_LOG_DIR`

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
- `stardive update`

`stardive update` detects install variants and picks an update strategy in this order:
1. crates.io install (`cargo install`) if detected
2. native pkgx install (`pkgx pkgm install`) if detected
3. script/unknown fallback (re-runs the GitHub installer one-liner)

## crates.io publish readiness

This workspace is set up to publish the CLI in two steps:

1. Publish shared core crate:

```bash
cargo publish -p stardive-core
```

2. Publish companion CLI crate:

```bash
cargo publish -p stardive
```

Local packaging checks before publish:

```bash
cargo package -p stardive-core
cargo package -p stardive
```

## Static content

- `installers/`: shell scripts served via `/v1/installers`
- `eternal/`: static long-lived resources served via `/v1/eternal`

## ONCE-compatible Docker image

According to ONCE compatibility requirements, the app image must:
- serve HTTP on port `80`
- expose a successful health endpoint at `/up`
- persist data in `/storage`

This repo includes a `Dockerfile` that does this by default.

Build:

```bash
docker build -t ghcr.io/YOUR_USER/stardive-api:latest .
```

Run:

```bash
docker run --rm \
  -p 80:80 \
  -v stardive-storage:/storage \
  ghcr.io/YOUR_USER/stardive-api:latest
```

Optional ONCE backup hooks are included at `/hooks/pre-backup` and `/hooks/post-restore`.

## Deploy script

Automate the server rollout flow (push -> remote pull/build/push -> `once update` -> health checks):

```bash
./scripts/deploy-stardive-server.sh
```

Use `./scripts/deploy-stardive-server.sh --help` for options like `--no-push`, custom host, branch, or image.
