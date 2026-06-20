# Standalone Deployment

Standalone deployment is suitable for evaluation, personal projects, small teams, and early production environments. It does not require Redis and is easier to operate and troubleshoot. Move to [cluster deployment](cluster.md) when you need multiple replicas or horizontal scaling.

## Docker Compose Deployment

Docker Compose is the recommended standalone deployment path. It does not require Rust, Node.js, or pnpm on the host.

```bash
docker compose up -d --build
```

It starts:

- `web`: frontend Nginx service that serves static assets and proxies backend API traffic.
- `backend`: backend API, worker, and scheduler.
- `postgres`: PostgreSQL database.

After startup, open:

```text
http://SERVER_IP:8080
```

The bootstrap wizard will guide you through the admin account, service mode, initial upstream, pricing, SMTP, and payment settings.

### Check Runtime Status

Check container status:

```bash
docker compose ps
```

You should normally see `postgres`, `backend`, and `web` in `running` or `healthy` state.

View all logs:

```bash
docker compose logs -f
```

View backend container logs only:

```bash
docker compose logs -f backend
```

If the bootstrap wizard or login page opens in the browser, the frontend, backend, and reverse proxy path are usually working.

### Common Commands

Stop services:

```bash
docker compose down
```

Rebuild and start:

```bash
docker compose up -d --build
```

Restart the backend container:

```bash
docker compose restart backend
```

## Source Deployment

Source deployment can be used for development or for manually managed production processes. Development mode is useful for debugging and trying the bootstrap flow; production mode uses release backend and scheduler builds plus a static frontend served by Nginx. Docker Compose is still the recommended production path for most standalone deployments.

### Prerequisites

Prepare these dependencies:

| Dependency | Recommended Version |
| --- | --- |
| PostgreSQL | 16 or compatible |
| Rust | 1.94 or newer |
| Node.js | 20 or compatible |
| pnpm | Version compatible with the frontend project |

Confirm the commands are available:

```bash
psql --version
cargo --version
node --version
pnpm --version
```

If `cargo --version` shows an older version such as 1.75, upgrade the Rust toolchain before running the backend or scheduler.

Create a dedicated NeoGate database user and database:

```bash
sudo -u postgres psql
```

Run this in `psql`:

```sql
CREATE USER neogate WITH PASSWORD 'change-me';
CREATE DATABASE neogate OWNER neogate;
\q
```

Use this PostgreSQL connection URL in the bootstrap wizard:

```text
postgres://neogate:change-me@localhost:5432/neogate
```

On first startup, if runtime configuration is incomplete, the backend enters bootstrap mode. The bootstrap page writes the database connection, site settings, and random secrets. You usually do not need to edit `.env` manually first.

### Development Mode

Development mode uses the Rust debug build and the Vite dev server.

| Service | Command | Default URL |
| --- | --- | --- |
| Backend | `cargo run -p neogate` | `http://127.0.0.1:8080` |
| Scheduler | `cargo run -p neogate-scheduler` | No HTTP URL |
| Frontend | `cd frontend && pnpm install && pnpm dev --host 0.0.0.0` | `http://SERVER_IP:5173` |

Open `http://SERVER_IP:5173`. The page will redirect to the bootstrap wizard. Complete runtime configuration, admin account setup, service mode, initial upstream, and optional SMTP settings. If saving runtime configuration asks for a restart, restart the backend and refresh the page.

### Production Mode

For manual production deployment, build the backend and scheduler in release mode and serve the frontend static files with Nginx. Keep both Rust processes running with systemd, supervisord, or another process manager.

Build the backend and scheduler:

```bash
cargo build --release -p neogate -p neogate-scheduler
```

Run the backend:

```bash
BIND_ADDR=127.0.0.1:8080 ./target/release/neogate
```

Run the scheduler:

```bash
./target/release/neogate-scheduler
```

You can also manage both processes with systemd. This example assumes the project is installed at `/opt/neogate` and has already been built in release mode from the repository root:

```ini
[Unit]
Description=NeoGate backend

[Service]
WorkingDirectory=/opt/neogate
Environment=BIND_ADDR=127.0.0.1:8080
Environment=RUST_LOG=info
Environment=NEOGATE_ENV_FILE=/opt/neogate/.env
ExecStart=/opt/neogate/deploy/systemd/start-neogate.sh
KillMode=control-group
TimeoutStopSec=30
Restart=always
StandardOutput=append:/var/log/neogate/backend.log
StandardError=append:/var/log/neogate/backend-error.log

[Install]
WantedBy=multi-user.target
```

Save it as `/etc/systemd/system/neogate.service`, then start it:

```bash
sudo mkdir -p /var/log/neogate
sudo systemctl daemon-reload
sudo systemctl enable --now neogate
sudo systemctl status neogate
```

Add logrotate for NeoGate logs to avoid unbounded log growth:

```bash
sudo tee /etc/logrotate.d/neogate >/dev/null <<'EOF'
/var/log/neogate/*.log {
    daily
    rotate 14
    compress
    missingok
    notifempty
    copytruncate
}
EOF
```

Build the frontend:

```bash
cd frontend
pnpm install
pnpm build
cd ..
```

After the build, serve `frontend/dist` with Nginx or another static web server. The repository includes `deploy/nginx/source-build.conf.example`; it assumes `/usr/share/nginx/html` for static files and proxies backend API and health-check paths to `http://127.0.0.1:8080`.

Use it like this:

```bash
sudo install -d /usr/share/nginx/html
sudo cp -r frontend/dist/. /usr/share/nginx/html/
sudo cp deploy/nginx/source-build.conf.example /etc/nginx/conf.d/neogate.conf
sudo nginx -t
sudo systemctl reload nginx
```

## Troubleshooting

- Services do not become `running` or `healthy`: start with `docker compose logs -f`.
- `backend` does not start: check PostgreSQL health and the database connection URL.
- The page does not open: confirm host port `8080` is free and check firewall or security group rules.
- Frontend cannot reach the backend in source deployment: confirm Nginx proxies to `http://127.0.0.1:8080` and the backend process is running.
