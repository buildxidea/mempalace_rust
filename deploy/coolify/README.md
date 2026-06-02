# Deploy mempalace_rust on Coolify

[Coolify](https://coolify.io/self-hosted) is an open-source, self-hosted
Heroku/Render alternative that you run on your own VPS. This template
deploys MemPalace as a Coolify *Application* backed by a Docker
Compose stack — Coolify handles TLS termination, persistent volume
provisioning, log aggregation, and the deploy webhook for you.

## What you get

- A public HTTPS endpoint serving the MemPalace REST API on port 3111
- A persistent Docker volume backing `/data` for SQLite DB storage
- An HTTP health-check at `/mpr/livez`

## One-time setup

1. **Open your Coolify dashboard** and click **+ New → Application**.
2. **Source**: pick *Public Repository*. Paste the repo URL.
   Branch: `main`.
3. **Build Pack**: select *Docker Compose*.
4. **Base Directory**: `deploy/coolify`
5. **Compose Path**: `docker-compose.yml`
6. Set a **Domain** in the form `https://<your-fqdn>:3111`.
7. Click **Deploy**.

## Capture the HMAC secret

Once the deploy logs show the service is up, open the application's
**Logs** tab and search for `MEMPALACE_HMAC_SECRET=`. Copy it into your
client environment. The secret is never printed again on subsequent boots.

## Verify the deployment

```bash
curl "https://<your-coolify-domain>/mpr/livez"
```

## Key environment variables

| Variable | Description |
|----------|-------------|
| `MEMPALACE_HMAC_SECRET` | Bearer token for authenticated API calls (auto-generated) |
| `DATA_DIR` | Path to persistent storage (default: `/data`) |