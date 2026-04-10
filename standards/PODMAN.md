# Podman

> Additive to STANDARDS.md, SYSTEMD.md, and OPERATIONS.md. Read those first. Everything here is container-specific.
>
> Covers: pod architecture, container naming, volume mounts (SELinux), health checks, auto-update timer, systemd integration, rootless vs rootful, image pinning, Containerfile patterns, networking, security.
>
> **Key decisions:** rootful with pods for service grouping, systemd `.service` units wrapping `podman run`, `:Z` on all volume mounts, health checks mandatory, `podman-auto-update.timer` for image updates, `io.containers.autoupdate=registry` label on every container, env files at `/etc/menos-*.env` with 0600 root:root.

---

## Pod architecture

Pods group related containers that share a network namespace. Containers within a pod communicate over `127.0.0.1`. Ports are published on the pod, not individual containers.

### When to use pods

| Pattern | Example | Reason |
|---------|---------|--------|
| Shared network namespace | AdGuard + NPM (gateway pod) | NPM proxies to AdGuard on `127.0.0.1:8080` within the pod |
| Co-scheduled services | Plex + Tautulli (media pod) | Tautulli monitors Plex; they scale together |
| Dashboard grouping | Homepage + Uptime Kuma (dashboard pod) | Related UI services, single port surface |
| Independent service | CrowdSec, ntopng, GreptimeDB | No shared-namespace benefit; standalone |

### Pod creation

```bash
# Create pod with all published ports
# WHY: ports belong to the pod, not individual containers
podman pod create --name gateway -p 53:53/tcp -p 53:53/udp -p 80:80 -p 81:81 -p 443:443 -p 3000:8080

# Add containers to the pod
podman run -d --pod gateway --name adguard ...
podman run -d --pod gateway --name npm ...
```

### Pod networking rules

- **Cross-pod communication:** Use the host LAN IP (`192.168.0.18`), never `localhost`
- **Same-pod communication:** Use `127.0.0.1` with the container's internal port
- **Port mapping:** Defined at pod creation; individual containers do not publish ports

| Scenario | Address | Example |
|----------|---------|---------|
| NPM proxying to AdGuard (same pod) | `127.0.0.1:8080` | `proxy.lan` -> `dns.lan` backend |
| Homepage calling Uptime Kuma (same pod) | `127.0.0.1:3001` | Widget data source |
| Uptime Kuma monitoring Plex (cross-pod) | `192.168.0.18:32400` | Health check target |

---

## Container naming

| Element | Convention | Example |
|---------|-----------|---------|
| Pod names | `kebab-case`, functional group | `gateway`, `media`, `dashboard` |
| Container names | `kebab-case`, service name | `adguard`, `npm`, `plex`, `tautulli` |
| Systemd units | `{name}-container.service` | `adguard-container.service` |
| Image references | Full registry path with tag or digest | `docker.io/adguard/adguardhome:v0.107.52` |
| Volume paths | `/data/{service}` on dedicated disk | `/data/adguard`, `/data/npm` |
| Env files | `/etc/menos-{service}.env` | `/etc/menos-npm.env` |

---

## Systemd integration

Every container runs as a systemd service. No manual `podman run` in production.

### Unit file template

```ini
# /etc/systemd/system/{name}-container.service
[Unit]
Description={Name} container
After=network-online.target
Wants=network-online.target
# WHY: pod must exist before container starts
Requires={pod-name}-pod.service
After={pod-name}-pod.service

[Service]
Type=simple
Restart=always
RestartSec=10s
TimeoutStartSec=300

# WHY: env files contain credentials, 0600 root:root
EnvironmentFile=-/etc/menos-%i.env

ExecStartPre=-/usr/bin/podman rm -f %N
ExecStart=/usr/bin/podman run \
    --name %N \
    --pod {pod-name} \
    --label io.containers.autoupdate=registry \
    --health-cmd="..." \
    --health-interval=30s \
    --health-retries=3 \
    --health-start-period=10s \
    -v /data/{service}/config:/opt/{service}/conf:Z \
    {image}:{tag}
ExecStop=/usr/bin/podman stop -t 30 %N
ExecStopPost=-/usr/bin/podman rm -f %N

[Install]
WantedBy=multi-user.target
```

### Pod unit file template

```ini
# /etc/systemd/system/{pod-name}-pod.service
[Unit]
Description={Pod-Name} pod
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStartPre=-/usr/bin/podman pod rm -f {pod-name}
ExecStart=/usr/bin/podman pod create --name {pod-name} \
    -p {port-mappings}
ExecStop=/usr/bin/podman pod stop {pod-name}
ExecStopPost=/usr/bin/podman pod rm -f {pod-name}

[Install]
WantedBy=multi-user.target
```

### Lifecycle commands

```bash
# Start/stop a service (restarts container, not pod)
sudo systemctl restart adguard-container

# View status across all containers
sudo podman ps --format "table {{.Names}} {{.Status}}"

# View logs
sudo journalctl -u adguard-container.service -f

# Rebuild after config change
sudo systemctl restart adguard-container
```

---

## Volume mounts

### SELinux labeling

All volume mounts require SELinux context flags. Omitting these causes silent permission denials on SELinux-enforcing systems (Fedora, RHEL).

| Flag | Meaning | Use |
|------|---------|-----|
| `:Z` | Private unshared label | Default for all mounts. Each container gets its own label. |
| `:z` | Shared label | Only when multiple containers must access the same path. |
| `:ro` | Read-only | Combine with `:Z` as `:ro,Z` for config files. |

```bash
# Standard data mount
-v /data/adguard/work:/opt/adguardhome/work:Z
-v /data/adguard/conf:/opt/adguardhome/conf:Z

# Read-only config mount
-v /etc/myapp.conf:/etc/myapp.conf:ro,Z

# NAS mount (already has NFS context, no :Z needed)
# WARNING: :Z on NFS mounts will fail or corrupt SELinux labels on the NFS server
-v /nas/media:/media:ro
```

### Volume layout

Service data lives on the dedicated data disk (`/data`), not the OS disk (`/`):

```
/data/
├── adguard/        # AdGuard Home config + work
│   ├── conf/
│   └── work/
├── npm/            # Nginx Proxy Manager
├── plex/
│   └── config/
├── tautulli/
├── homepage/
│   └── config/
├── uptime-kuma/
├── crowdsec/
├── ntopng/
│   └── redis/      # NOTE: uid 101:102 required or credentials lost on restart
├── ntfy/
├── greptimedb/
└── vector/
```

### Ownership and permissions

```bash
# Most containers run as non-root internally; match UIDs
# NOTE: check the image docs for expected UID/GID

# ntopng Redis needs specific ownership
sudo chown -R 101:102 /data/ntopng/redis

# Secrets and env files are root-only
sudo chmod 0600 /etc/menos-*.env
sudo chown root:root /etc/menos-*.env
```

---

## Health checks

Every long-running container defines a health check. Containers without health checks fail silently.

### Health check patterns

| Service type | Health check | Example |
|-------------|-------------|---------|
| HTTP service | HTTP GET to health/status endpoint | `curl -f http://localhost:8080/health` |
| DNS service | DNS query | `nslookup localhost 127.0.0.1` |
| Database | Client connection test | `greptimedb-cli health` |
| Queue/stream | Process check | `pgrep -f vector` |

### Containerfile HEALTHCHECK

```dockerfile
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/health || exit 1
```

### Runtime health check flags

```bash
podman run \
    --health-cmd="curl -f http://localhost:8080/health || exit 1" \
    --health-interval=30s \
    --health-retries=3 \
    --health-start-period=10s \
    --health-timeout=3s \
    myimage:tag
```

### Monitoring health

```bash
# Check individual container health
podman healthcheck run adguard

# View health status in ps output
podman ps --format "table {{.Names}} {{.Status}}"

# Uptime Kuma monitors use LAN IPs, not Tailscale
# WHY: Uptime Kuma runs on the same LAN as the services
```

---

## Auto-update

The `podman-auto-update.timer` pulls new images and restarts containers with the `io.containers.autoupdate=registry` label.

### Setup

```bash
# Enable the timer (runs weekly, Sundays 4am by default)
sudo systemctl enable --now podman-auto-update.timer

# Verify timer schedule
systemctl list-timers podman-auto-update.timer
```

### Requirements

Every container that should auto-update must have:

1. **The autoupdate label:**
   ```bash
   --label io.containers.autoupdate=registry
   ```

2. **A fully qualified image reference (not `latest`):**
   ```bash
   # Valid: tag or digest
   docker.io/adguard/adguardhome:v0.107.52
   docker.io/library/nginx:1.27@sha256:abc123...

   # Invalid: bare name or latest
   adguardhome
   nginx:latest
   ```

### Manual trigger

```bash
# Dry run (check for updates without applying)
sudo podman auto-update --dry-run

# Apply updates
sudo podman auto-update
```

---

## Image pinning

### Tag strategy

| Environment | Strategy | Example |
|-------------|----------|---------|
| Production services | Minor version pin | `adguardhome:v0.107` |
| Critical infrastructure | Patch version pin | `adguardhome:v0.107.52` |
| Maximum reproducibility | Digest pin | `adguardhome@sha256:abc123...` |
| Build stages only | Major version pin | `rust:1.78-bookworm` |

Never use `latest` in systemd unit files. The tag must be explicit and auditable.

### Containerfile pinning

```dockerfile
# Build stage: major version pin is acceptable
FROM rust:1.78-bookworm AS builder

# Runtime stage: pin to digest for reproducibility
FROM gcr.io/distroless/cc-debian12@sha256:abc123...
```

---

## Rootless vs rootful

### Decision matrix

| Requirement | Rootless | Rootful |
|-------------|----------|---------|
| Bind ports < 1024 (53, 80, 443) | No | **Yes** |
| Device access (GPU, USB) | No | **Yes** |
| NFS mount access | Depends | **Yes** |
| User namespace isolation | **Yes** | No |
| No root on host | **Yes** | No |
| Development/testing | **Yes** | Either |

### Homelab pattern

Homelab services run rootful because they bind low ports (53, 80, 443) and access NFS mounts. Systemd units use system scope (`/etc/systemd/system/`), not user scope.

```bash
# Rootful: system-level service
sudo systemctl start adguard-container.service

# Rootless: user-level service (development)
systemctl --user start dev-api.service
```

### Rootless prerequisites

When running rootless containers:

```bash
# Enable lingering (containers survive logout)
loginctl enable-linger $USER

# Enable user podman socket
systemctl --user enable --now podman.socket

# Storage locations
# Images: ~/.local/share/containers/storage
# Runtime: /run/user/$(id -u)/containers
# Config: ~/.config/containers
```

---

## Containerfile guidelines

### Multi-stage builds

Separate build and runtime concerns:

```dockerfile
FROM rust:1.78-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/myapp /bin/
ENTRYPOINT ["/bin/myapp"]
```

### Base image priority

| Priority | Source | Example |
|----------|--------|---------|
| 1 | Distroless | `gcr.io/distroless/static-debian12` |
| 2 | Minimal official | `alpine:3.19` |
| 3 | Slim variants | `debian:12-slim` |
| 4 | Full official | `rust:1.78-bookworm` (build only) |

### Required metadata

```dockerfile
LABEL org.opencontainers.image.source="https://github.com/org/repo"
LABEL org.opencontainers.image.revision="${GIT_REVISION}"
LABEL org.opencontainers.image.version="${VERSION}"
```

### Layer ordering

Order by change frequency (least-changing first): base image, system deps, user creation, workdir, dependency manifests, dependency install, source copy, build, runtime config (`USER`, `EXPOSE`, `HEALTHCHECK`, `ENTRYPOINT`).

---

## Networking

### Network types

| Type | Use |
|------|-----|
| Pod networking | Default for grouped services (shared `localhost`) |
| `bridge` | Container-to-container on same host (standalone containers) |
| `pasta` | Rootless default on Podman 5+ |
| `slirp4netns` | Rootless fallback |
| `host` | Avoid unless required (breaks isolation) |

### Custom networks for standalone containers

```bash
podman network create --driver bridge internal-net
podman run --network internal-net --name crowdsec ...
```

### Port publishing

```bash
# Bind to all interfaces (default for services)
podman run -p 8080:8080 myapp

# Bind to specific interface
podman run -p 127.0.0.1:8080:8080 myapp

# UDP (required for DNS)
podman run -p 53:53/udp dns-server
```

---

## Security

### Capability management

```bash
# Drop all, add only what's needed
podman run --cap-drop=ALL --cap-add=NET_BIND_SERVICE myapp
```

### Runtime hardening

```bash
podman run \
    --security-opt=no-new-privileges \
    --read-only \
    --tmpfs /tmp \
    --tmpfs /var/cache \
    myapp
```

### Secrets

```bash
# Podman secrets (preferred over env vars for credentials)
printf 'mysecret' | podman secret create db_password -
podman run --secret db_password,target=/run/secrets/db_password myapp

# Env file approach (used in homelab)
# WHY: systemd EnvironmentFile is simpler for service-level config
# Env files at /etc/menos-*.env, 0600 root:root
```

### Container user

```dockerfile
# Create non-root user in Containerfile
RUN addgroup --gid 1000 appgroup && \
    adduser --uid 1000 --ingroup appgroup --disabled-password appuser
USER appuser:appgroup
```

---

## Anti-patterns

| Anti-pattern | Problem | Fix |
|-------------|---------|-----|
| `--privileged` | Bypasses all security | Use specific `--cap-add` |
| Running as root in container | Unnecessary privilege | `USER` directive in Containerfile |
| `latest` tag in unit files | Non-deterministic, breaks auto-update | Pin to version tag |
| Missing `:Z` on Fedora/RHEL | Silent permission denial | Always use `:Z` for bind mounts |
| `:Z` on NFS mounts | Corrupts SELinux labels on NFS server | Omit SELinux flags for NFS |
| Ports on containers in pods | Ignored; ports belong to the pod | Publish ports at pod creation |
| `localhost` for cross-pod calls | Pods have isolated network namespaces | Use host LAN IP |
| Missing health checks | Silent failures | `--health-cmd` on every container |
| Manual `podman run` in production | Not reproducible, no restart | Wrap in systemd unit |
| Docker socket mounting | Massive security risk | Use Podman API socket if needed |
| No resource limits | Unbounded containers exhaust host | Set `--memory` and `--cpus` |
| Env vars for secrets | Visible in `podman inspect` | Use Podman secrets or mounted files |

---

## Tooling

| Tool | Purpose |
|------|---------|
| `podman` | Container runtime (daemonless, OCI-compliant) |
| `buildah` | Low-level image building |
| `skopeo` | Image inspection and registry operations |
| `podman-compose` | Docker Compose compatibility layer |
| `systemctl` | Service lifecycle for container units |
| `journalctl -u {name}-container` | Container service logs |
| `podman healthcheck run {name}` | Manual health check trigger |
| `podman auto-update --dry-run` | Check for available image updates |

---

## Cross-references

| Topic | Standard |
|-------|----------|
| Systemd unit patterns | SYSTEMD.md |
| Reverse proxy configuration | NGINX.md |
| Backup of service data | RESTIC.md |
| Monitoring and health checks | OPERATIONS.md#monitoring |
| Credential handling | SECURITY.md |
| DNS and firewall | SECURITY.md#dns, OPERATIONS.md |
