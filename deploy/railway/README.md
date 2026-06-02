# Deploy mempalace_rust on Railway

This template runs MemPalace (Rust) on a single Railway service with a
persistent volume mounted at `/data`. The HMAC secret is generated on
first boot and persisted to the volume — you read it once from the
deploy logs and copy it into your client.

## What you get

- A public HTTPS endpoint serving the MemPalace REST API on port 3111
- A persistent Railway Volume at `/data` for SQLite DB storage
- Railway healthcheck against `/mpr/livez`

## Deploy via Railway dashboard

1. Click **Deploy from GitHub** in the Railway dashboard and pick the repo.
2. Set the **Config-as-Code Path** under the service Settings to
   `deploy/railway/railway.json`. Railway picks up the Dockerfile path from there.
3. Open the service's **Volumes** tab and add a volume mounted at `/data`.
4. Click **Deploy**.

## Deploy via Railway CLI

```bash
railway login
railway init
railway up --service mempalace
railway volume add --service mempalace --mount /data
railway redeploy
```

## Capture the HMAC secret

```bash
railway logs --service mempalace | grep MEMPALACE_HMAC_SECRET=
```

## Verify the deployment

```bash
curl https://<your-service>.up.railway.app/mpr/livez
```

## Key environment variables

| Variable | Description |
|----------|-------------|
| `MEMPALACE_HMAC_SECRET` | Bearer token for authenticated API calls (auto-generated) |
| `DATA_DIR` | Path to persistent storage (default: `/data`) |