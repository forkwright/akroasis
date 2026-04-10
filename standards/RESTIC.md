# Restic

> Standards for backup operations using restic. Applies to all systems running automated or manual backups.

---

## Repository setup

### Location

Restic repositories are target-mounted, not local. Local repositories are prohibited -- they fail when the disk fails.

| Environment | Repository Path | Rationale |
|-------------|-----------------|-----------|
| Homelab | `/nas/docker/backups/{hostname}` | NAS is the source of truth for backups |
| Cloud | `s3:{bucket}/backups/{hostname}` | Object storage for ephemeral instances |

### Initialization

Initialize once per host. Document the repository location in the host's CLAUDE.md.

```bash
# Initialize with explicit repo path (never rely on defaults)
restic -r /nas/docker/backups/menos init

# Or with environment file
RESTIC_REPOSITORY=/nas/docker/backups/menos
RESTIC_PASSWORD_FILE=~/secrets/restic-password
restic init
```

### Password management

- **Never** pass passwords via command line (visible in `ps`)
- **Never** store passwords in backup scripts
- Use `RESTIC_PASSWORD_FILE` pointing to a file with `0600` permissions
- Validate permissions at runtime: fail if readable by group/other

```bash
# Runtime permission check
if [[ "$(stat -c '%a' "$RESTIC_PASSWORD_FILE")" != "600" ]]; then
    echo "ERROR: Password file must have 0600 permissions"
    exit 1
fi
```

---

## Backup operations

### Scope

Back up what matters, not everything:

| Include | Rationale |
|---------|-----------|
| User configs (`~/.config/`, dotfiles) | Hard to recreate |
| Systemd units and timers | Service definitions |
| Service configs (`/etc/menos-*.env`) | Runtime configuration |
| SSH keys and auth | Identity |
| `/theke/` or equivalent knowledge store | Institutional memory |

| Exclude | Rationale |
|---------|-----------|
| Container images | Re-pullable from registry |
| Build artifacts (`target/`, `node_modules/`) | Reproducible from source |
| Bulk media data | Already on NAS (source of truth) |
| `~/.cache/` | Transient |

### Command structure

```bash
# Standard backup invocation
restic backup \
    --tag "$(date +%Y%m%d-%H%M%S)" \
    --exclude-file=/etc/restic/excludes \
    --one-file-system \
    /home/ck /etc/menos-ops /etc/systemd/system
```

Flags:
- `--one-file-system`: Prevents crossing into NFS mounts accidentally
- `--exclude-file`: Centralized exclusion list, version-controlled
- `--tag`: Timestamp tags for easy selection

### Snapshot verification

Verify after every backup. A backup you can't restore is worthless.

```bash
# Quick integrity check (metadata only)
restic check

# Full data verification (slow, do weekly)
restic check --read-data
```

---

## Retention policy

### Standard schedule

| Snapshot type | Count | When to prune |
|---------------|-------|---------------|
| Daily | 7 | Every day |
| Weekly | 4 | Every week |
| Monthly | 3 | Every month |

### Prune execution

```bash
# Apply retention policy
restic forget \
    --keep-daily 7 \
    --keep-weekly 4 \
    --keep-monthly 3 \
    --prune
```

`--prune` reclaims space immediately. Without it, data is marked for deletion but retained.

---

## Automation

### Systemd timer (preferred)

```ini
# ~/.config/systemd/user/restic-backup.service
[Unit]
Description=Daily restic backup
After=network-online.target

[Service]
Type=oneshot
EnvironmentFile=%h/menos-ops/secrets/restic-env
ExecStartPre=/bin/sh -c 'test "$(stat -c %%a %h/menos-ops/secrets/restic-env)" = "600"'
ExecStart=/usr/local/bin/restic-backup.sh
ExecStartPost=/usr/local/bin/restic-verify.sh
```

```ini
# ~/.config/systemd/user/restic-backup.timer
[Unit]
Description=Run backup daily at 3 AM

[Timer]
OnCalendar=*-*-* 03:00:00
Persistent=true

[Install]
WantedBy=timers.target
```

Enable: `systemctl --user enable restic-backup.timer`

### Cron (acceptable)

```bash
# crontab entry
0 3 * * * /usr/local/bin/restic-backup.sh 2>&1 | logger -t restic-backup
```

Cron requires manual log rotation handling. Prefer systemd for log management.

---

## Monitoring and alerting

### Failure notification

Backup failures must surface immediately. Silent failures accumulate until data loss.

```bash
#!/bin/bash
# restic-backup.sh - wrapper with ntfy notification

set -euo pipefail

# WHY: trap ensures notification on any exit path
cleanup() {
    local exit_code=$?
    if [[ $exit_code -ne 0 ]]; then
        curl -H "Authorization: Bearer $NTFY_TOKEN" \
             -d "Backup failed on $(hostname) with exit $exit_code" \
             ntfy.sh/alerts
    fi
}
trap cleanup EXIT

# Actual backup
restic backup --exclude-file=/etc/restic/excludes /home/ck /etc/menos-ops
restic forget --keep-daily 7 --keep-weekly 4 --keep-monthly 3 --prune
```

### Metrics to track

| Metric | Collection method | Alert threshold |
|--------|-------------------|-----------------|
| Backup duration | systemd timer logs | >30 min |
| Snapshot count | `restic snapshots | wc -l` | <3 (indicates failure) |
| Repository size | `restic stats` | >80% of target capacity |
| Last backup age | `restic snapshots --json | jq` | >25 hours |

---

## Recovery procedures

### Full restore

```bash
# List available snapshots
restic snapshots

# Restore entire snapshot to temporary location
restic restore latest --target /tmp/restore-$(date +%s)

# Restore specific file
restic restore latest --target /tmp/restore --include /home/ck/.ssh/id_ed25519
```

Always restore to a temporary location first. Verify before replacing live files.

### Lock recovery

Stale locks block operations after unclean termination:

```bash
# Check for locks
restic list locks

# Remove stale lock (verify no other process is running first)
restic unlock
```

Document this procedure in host CLAUDE.md. Agents encountering lock errors must check CLAUDE.md before proceeding.

---

## Security

### Transport encryption

- NAS mounts: Rely on NFS encryption (WireGuard tunnel) or local network trust
- Cloud: Restic encrypts client-side; transport via HTTPS

### At-rest encryption

Restic encrypts by default. No additional configuration needed.

### Key rotation

Restic does not support key rotation. To rotate:
1. Initialize new repository with new password
2. Backup to new repository
3. Verify restore from new repository
4. Remove old repository

### Permission model

| File | Mode | Owner |
|------|------|-------|
| Backup script | 0755 | root or user |
| Password file | 0600 | User running backup |
| Exclude list | 0644 | root or user |
| Restic binary | 0755 | root |

---

## Troubleshooting

### Stale lock files

**Symptom**: `unable to create lock in backend: repository is already locked`

**Resolution**:
```bash
# Verify no backup is running
ps aux | grep restic

# Unlock
restic unlock
```

**Prevention**: Use `trap EXIT` in scripts to ensure cleanup on interruption.

### Permission denied on restore

**Symptom**: Restored files have wrong ownership

**Cause**: Restic preserves UIDs/GIDs, which may not map on new system

**Fix**: Restore with `--numeric-ids` or `chown` after restore

### Repository corruption

**Symptom**: `checksum mismatch` or `cipher: message authentication failed`

**Immediate action**: Do not prune. Check repository on different machine.

**Recovery**:
```bash
# Attempt repair (may lose snapshots)
restic repair index
restic repair snapshots
```

---

## Anti-patterns

1. **Plaintext passwords in scripts**: Use `RESTIC_PASSWORD_FILE`
2. **Local-only backups**: Always replicate to separate hardware
3. **Unverified backups**: Test restores quarterly
4. **Infinite retention**: Prune old snapshots to control size
5. **Crossing filesystem boundaries**: Use `--one-file-system` to avoid backup loops
6. **Ignoring exit codes**: Wrap in scripts with `set -e` and monitoring
