# Deploy mempalace_rust on Fly.io

This template runs MemPalace (Rust) on a single Fly.io machine with a 1 GB
persistent volume mounted at `/data`. The HMAC secret is generated on
first boot and persisted to the volume — you capture it from the deploy
logs exactly once.

## What you get

- A public HTTPS endpoint serving the MemPalace REST API on port 3111
- A 1 GB Fly Volume at `/data` for SQLite DB storage
- `auto_stop_machines = "stop"` and `min_machines_running = 0` — the
  machine sleeps when idle, so cost floor approaches $0 for low traffic
- HTTP healthcheck at `/mpr/livez` every 30 s

## One-time setup

```bash
# Install flyctl: https://fly.io/docs/flyctl/install/
# Pick your unique app name:
export APP="mempalace-$(whoami)"

# From the repo root:
fly launch --copy-config --no-deploy --name "$APP"
fly volumes create "${APP//-/_}_data" --region iad --size 1
fly deploy --app "$APP"
```

## Capture the HMAC secret

```bash
fly logs --app "$APP" | grep MEMPALACE_HMAC_SECRET=
```

## Verify the deployment

```bash
curl "https://$APP.fly.dev/mpr/livez"
```

## Key environment variables

| Variable | Description |
|----------|-------------|
| `MEMPALACE_HMAC_SECRET` | Bearer token for authenticated API calls (auto-generated) |
| `DATA_DIR` | Path to persistent storage (default: `/data`) |