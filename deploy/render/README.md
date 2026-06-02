# Deploy mempalace_rust on Render

This template runs MemPalace (Rust) on a single Render Web Service with a
persistent disk mounted at `/data`. The HMAC secret is generated on
first boot and persisted to the disk — you capture it from the deploy
logs exactly once.

## What you get

- A public HTTPS endpoint serving the MemPalace REST API on port 3111
- A 1 GB persistent disk at `/data` for SQLite DB storage
- Render healthcheck against `/mpr/livez`

## Deploy via Render Blueprint

1. Push the `deploy/render/` directory to a Git provider Render can reach.
2. In the Render dashboard, click **New +** → **Blueprint**.
3. Point Render at the repo and the path `deploy/render/render.yaml`.
4. Render reads the Blueprint, provisions the disk, builds the
   Dockerfile, and starts the service.

## Verify the deployment

```bash
curl https://<your-service>.onrender.com/mpr/livez
```

## Capture the HMAC secret

After the first deploy, check the logs for `MEMPALACE_HMAC_SECRET=`.

## Key environment variables

| Variable | Description |
|----------|-------------|
| `MEMPALACE_HMAC_SECRET` | Bearer token for authenticated API calls (auto-generated) |
| `DATA_DIR` | Path to persistent storage (default: `/data`) |
| `PORT` | Container port (default: `3111`) |