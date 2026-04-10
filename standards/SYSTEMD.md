# Systemd

> Additive to STANDARDS.md and OPERATIONS.md. Read those first. Everything here is systemd-specific.
>
> Covers: service units, timer units, security hardening, resource limits, journald logging, socket activation.
>
> **Key decisions:** service files in `config/systemd/`, `Type=notify` with sd-notify, `Restart=on-failure`, `DynamicUser=yes` where possible, hardening by default, structured JSON logging to journald.

---

## Service file structure

### Location

Service files live in `config/systemd/` at the project root. Copy or symlink to `/etc/systemd/system/` during deployment.

```
config/
  systemd/
    myapp.service
    myapp.timer
    myapp.socket
```

### Naming

| File | Convention | Example |
|------|------------|---------|
| Service unit | `kebab-case.service` | `kanon-dispatcher.service` |
| Timer unit | `kebab-case.timer` | `kanon-backup.timer` |
| Socket unit | `kebab-case.socket` | `kanon-api.socket` |

### Minimal service template

```ini
[Unit]
Description=My Application
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
ExecStart=/usr/local/bin/myapp serve
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5s

# Security
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/myapp

# Resource limits
MemoryMax=512M
TasksMax=100

# Environment
Environment="RUST_LOG=info"
Environment="RUST_BACKTRACE=1"
EnvironmentFile=-/etc/default/myapp

[Install]
WantedBy=multi-user.target
```

---

## Service types

| Type | Use | When |
|------|-----|------|
| `simple` | Foreground process, no readiness signal | Default for most services |
| `notify` | Service signals readiness via sd-notify | **Preferred**: enables proper dependency ordering |
| `exec` | Like simple, but waits for exec() | When you need fork safety |
| `oneshot` | Single-shot commands | Migrations, initialization tasks |
| `dbus` | D-Bus activated | Services exposing D-Bus interfaces |

### Type=notify implementation

Use the `sd-notify` protocol to signal readiness and watchdog keepalives:

```rust
// Rust with sd-notify crate or manual implementation
use std::os::unix::net::UnixDatagram;
use std::env;

fn notify_ready() {
    if let Some(notify_socket) = env::var_os("NOTIFY_SOCKET") {
        let socket = UnixDatagram::unbound().ok()?;
        let _ = socket.send_to(b"READY=1\n", notify_socket);
    }
}
```

Services must signal `READY=1` before systemd considers dependencies satisfied. Critical for services that other units depend on.

---

## Security hardening

Apply hardening by default. Remove only when the service genuinely requires the capability.

### Essential options (apply to all services)

```ini
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
```

### Capability bounding

```ini
# Drop all capabilities, add only what's needed
CapabilityBoundingSet=
AmbientCapabilities=

# Example: needs to bind low ports
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
AmbientCapabilities=CAP_NET_BIND_SERVICE
```

### Namespace isolation

```ini
# Run as dynamic user (no system user needed)
DynamicUser=yes

# Or explicit user/group
User=myapp
Group=myapp

# Private directories
PrivateTmp=yes
PrivateDevices=yes
PrivateUsers=yes
```

### Filesystem restrictions

```ini
ProtectSystem=strict      # Read-only root filesystem
ProtectHome=yes           # No access to /home
ReadWritePaths=/var/lib/myapp /var/log/myapp
ReadOnlyPaths=/etc/myapp
InaccessiblePaths=/proc/sys /sys
```

### Network restrictions

```ini
# Fully isolated
PrivateNetwork=yes

# Or restrict to specific addresses
IPAddressDeny=any
IPAddressAllow=10.0.0.0/8
IPAddressAllow=127.0.0.1

# Disable specific protocols
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
```

### System call filtering

```ini
# Allow only common safe syscalls (recommended)
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM

# Or allowlist specific syscalls
SystemCallFilter=~@mount @privileged @debug @cpu-emulation
```

---

## Resource management

### Memory limits

```ini
MemoryMax=512M            # Hard limit: OOM kill if exceeded
MemoryHigh=400M           # Soft limit: throttle before hard limit
MemorySwapMax=0           # Disable swap usage
```

### CPU limits

```ini
CPUQuota=80%              # Max 80% of one core
CPUWeight=100             # 1-10000, default 100
AllowedCPUs=0-3           # Pin to specific cores
```

### Task limits

```ini
TasksMax=100              # Prevent fork bombs
LimitNOFILE=65536         # File descriptor limit
LimitNPROC=100            # Max processes
```

### I/O limits

```ini
IOWeight=100              # 1-10000, default 100
IOReadBandwidthMax=/var 10M
IOWriteBandwidthMax=/var 10M
```

---

## Restart and failure policies

### Restart configuration

```ini
Restart=on-failure        # Restart on non-zero exit, unclean signal, timeout, watchdog
RestartSec=5s             # Wait before restart
StartLimitInterval=60s    # Time window for start limit
StartLimitBurst=3         # Max starts within interval
```

| Restart value | Behavior |
|---------------|----------|
| `no` | Never restart |
| `on-success` | Restart on clean exit (0) |
| `on-failure` | **Default**: Restart on unclean exit |
| `on-abnormal` | Restart on signal/timeout/watchdog |
| `on-abort` | Restart on unclean signal |
| `always` | Always restart |

### Failure escalation

```ini
[Unit]
OnFailure=failure-handler@%n.service

[Service]
Restart=on-failure
RestartSec=10s
StartLimitInterval=300s
StartLimitBurst=5
```

Create `failure-handler@.service` to handle failures (alerts, cleanup, etc.).

---

## Logging

### Journald integration

Services log structured data to stderr/stdout. Journald captures and enriches:

```ini
[Service]
StandardOutput=journal
StandardError=journal
SyslogIdentifier=myapp
```

### Structured logging from Rust

```rust
// tracing-journald for native structured logging
tracing_subscriber::fmt()
    .json()
    .with_writer(std::io::stderr)
    .init();
```

### Log filtering

```ini
Environment="RUST_LOG=info,myapp=debug"
```

### Journald persistence

```bash
# /etc/systemd/journald.conf
[Journal]
Storage=persistent
SystemMaxUse=500M
MaxFileSec=1week
```

### Querying logs

```bash
journalctl -u myapp.service -f                    # Follow
journalctl -u myapp.service --since "1 hour ago"  # Time range
journalctl -u myapp.service -p err                # Errors only
journalctl --user -u myapp.service                # User service
```

---

## Timer units (cron replacement)

Prefer systemd timers over cron for:
- Dependency ordering (timer triggers service after dependencies)
- Logging (all output in journal)
- Resource control (inherits service limits)
- Failure handling (restart policies apply)

### Timer template

```ini
# myapp-backup.timer
[Unit]
Description=MyApp backup timer

[Timer]
OnCalendar=daily
Persistent=true              # Run immediately if missed
RandomizedDelaySec=1hour     # Spread load

[Install]
WantedBy=timers.target
```

```ini
# myapp-backup.service
[Unit]
Description=MyApp backup

[Service]
Type=oneshot
ExecStart=/usr/local/bin/myapp backup
User=myapp
```

### Calendar syntax

| Expression | Meaning |
|------------|---------|
| `daily` | Every midnight |
| `hourly` | Every hour |
| `weekly` | Monday 00:00 |
| `*:*:00` | Every minute |
| `Mon *-*-1..7 02:00` | First Monday, 2 AM |
| `Mon,Fri 08:00` | Monday and Friday at 8 AM |

Enable with: `systemctl enable --now myapp-backup.timer`

---

## Socket activation

Services can be socket-activated: systemd listens on the port, starts the service on first connection.

### Socket unit

```ini
# myapp.socket
[Unit]
Description=MyApp socket

[Socket]
ListenStream=8080
BindIPv6Only=both
NoDelay=true

[Install]
WantedBy=sockets.target
```

### Service unit

```ini
# myapp.service
[Unit]
Description=MyApp
Requires=myapp.socket

[Service]
Type=notify
ExecStart=/usr/local/bin/myapp serve
```

### Receiving the socket

```rust
use std::os::unix::io::FromRawFd;
use std::net::TcpListener;

fn activate_socket() -> Option<TcpListener> {
    let fds = sd_listen_fds()?;
    if fds > 0 {
        // fd 3 is the first passed socket
        Some(unsafe { TcpListener::from_raw_fd(3) })
    } else {
        None
    }
}
```

Benefits:
- Service starts on demand
- Zero-downtime restarts (socket stays open)
- Dependency ordering: socket ready before service starts

---

## Deployment

### Installation

```bash
# Copy service files
sudo cp config/systemd/*.service /etc/systemd/system/
sudo cp config/systemd/*.timer /etc/systemd/system/

# Reload daemon
sudo systemctl daemon-reload

# Enable services
sudo systemctl enable myapp.service
sudo systemctl enable myapp-backup.timer

# Start
sudo systemctl start myapp.service
sudo systemctl start myapp-backup.timer
```

### Verification

```bash
systemctl status myapp.service           # Service state
systemctl is-active myapp.service        # Exit 0 if active
systemctl show myapp.service --property=MainPID
systemd-analyze security myapp.service   # Security score
systemd-analyze verify myapp.service     # Validate unit file
```

### Reload vs restart

| Command | Use |
|---------|-----|
| `systemctl reload` | Config reload (SIGHUP), no downtime |
| `systemctl restart` | Full process restart |
| `systemctl try-restart` | Restart only if already running |

---

## Anti-patterns

1. **Missing `Type=notify`**: Services signal readiness too early, breaking dependency chains
2. **`Restart=always` on failing service**: Infinite crash loop, no backoff
3. **No resource limits**: Memory leaks consume all RAM, affecting the whole system
4. **`User=root` by default**: Run with least privilege
5. **Hardcoded paths**: Use `EnvironmentFile` for deployment-specific values
6. **Missing `After=network-online.target`**: Service starts before network is ready
7. **No `ExecReload`**: Forces restart for config changes
8. **Logging to files**: Use journald, avoid log rotation complexity
9. **Cron jobs without logging**: Silent failures; use timers with `StandardError=journal`
10. **`KillMode=none`**: Orphaned processes on stop; use `KillMode=mixed` or `control-group`

---

## Tooling

| Command | Purpose |
|---------|---------|
| `systemctl status <unit>` | Service state and recent logs |
| `systemctl cat <unit>` | Display effective unit file |
| `systemctl edit --full <unit>` | Edit unit file override |
| `journalctl -u <unit>` | View service logs |
| `systemd-analyze security <unit>` | Security exposure score |
| `systemd-analyze verify <unit>` | Validate unit file syntax |
| `systemd-delta` | Show overridden configuration |
