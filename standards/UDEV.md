# Udev

> Additive to STANDARDS.md. Read that first. Everything here is udev-specific.
>
> Covers: udev rules for hardware detection, USB serial devices, device permissions, systemd integration.
>
> **Key decisions:** 99-prefix for custom rules, dialout group for serial, TAG+="systemd" for service binding, ATTRS over ATTR for parent matching, udevadm for testing.

---

## Rule syntax

Udev rules are declarative condition-action pairs. One rule per line. Conditions are AND-combined unless `,` is replaced with `|`. Actions are comma-separated after the `=` or `+=` operator.

### Rule structure

```
CONDITION1, CONDITION2, ... ACTION1, ACTION2, ...
```

Conditions match device attributes. Actions set properties, symlinks, or run commands.

### Common condition keys

| Key | Matches | Example |
|-----|---------|---------|
| `SUBSYSTEM` | Kernel subsystem | `SUBSYSTEM=="tty"` |
| `KERNEL` | Device name | `KERNEL=="ttyUSB*"` |
| `ATTR{name}` | sysfs attribute | `ATTR{idVendor}=="067b"` |
| `ATTRS{name}` | Parent device attribute (walks up tree) | `ATTRS{idVendor}=="067b"` |
| `ENV{key}` | Environment variable | `ENV{ID_BUS}=="usb"` |
| `TAGS` | Device tag | `TAGS=="systemd"` |
| `ACTION` | add/remove/change/bind/unbind | `ACTION=="add"` |

### Common action keys

| Key | Effect | Example |
|-----|--------|---------|
| `MODE` | File permissions | `MODE="0660"` |
| `GROUP` | Device group | `GROUP="dialout"` |
| `OWNER` | Device owner | `OWNER="root"` |
| `SYMLINK+=` | Create additional symlinks | `SYMLINK+="radio_%n"` |
| `TAG+=` | Add systemd tag | `TAG+="systemd"` |
| `ENV{SYSTEMD_WANTS}+=` | Trigger service start | `ENV{SYSTEMD_WANTS}+="radio-daemon.service"` |
| `RUN+=` | Execute command (use sparingly) | `RUN+="/usr/bin/logger new device"` |

---

## File naming

Rules live in `/etc/udev/rules.d/` (custom) or `/lib/udev/rules.d/` (package defaults). Files are processed in lexical order.

### Naming convention

| Prefix | Purpose | Example |
|--------|---------|---------|
| `50-` | Hardware defaults | `50-usb-serial.rules` |
| `70-` | Distribution defaults | `70-persistent-net.rules` |
| `99-` | **Site-local overrides** | `99-radio-devices.rules` |

**Use `99-` prefix for all custom rules.** This ensures they override distribution defaults. Single digits before the hyphen; `100-` sorts before `99-`.

---

## USB serial device rules

USB-to-serial adapters require matching on parent USB device attributes (VID:PID) while operating on the tty child.

### Pattern for USB serial devices

```udev
# Match parent USB device attributes, apply to tty child
SUBSYSTEM=="tty", ATTRS{idVendor}=="067b", ATTRS{idProduct}=="2303", 
    GROUP="dialout", MODE="0660", SYMLINK+="radio_pl2303_%n"

# CH340 cable
SUBSYSTEM=="tty", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", 
    GROUP="dialout", MODE="0660", SYMLINK+="radio_ch340_%n"

# CP2102 cable
SUBSYSTEM=="tty", ATTRS{idVendor}=="10c4", ATTRS{idProduct}=="ea60", 
    GROUP="dialout", MODE="0660", SYMLINK+="radio_cp2102_%n"
```

### Key requirements

- Use `ATTRS` (not `ATTR`) for VID/PID matching. `ATTR` only checks the immediate device; `ATTRS` walks up the device tree to find USB attributes.
- Always set `GROUP="dialout"` for serial devices. Users need dialout group membership to access.
- Use `%n` in symlinks for enumeration (ttyUSB0 → radio_pl2303_0).

### Common USB serial chips

| Chip | VID:PID | Notes |
|------|---------|-------|
| Prolific PL2303 | `067b:2303` | Most common. Clones are rampant but work on Linux. |
| WinChipHead CH340 | `1a86:7523` | Reliable, cheap. |
| Silicon Labs CP2102 | `10c4:ea60` | Higher-end cables. |
| FTDI FT232R | `0403:6001` | Less common for Baofeng. |

---

## Permissions

Serial devices default to `root:root` with mode `0600`. Applications need access.

### Required permission pattern

```udev
GROUP="dialout", MODE="0660"
```

### User membership

Users must be in the `dialout` group:

```bash
sudo usermod -aG dialout $USER
# Re-login required for group change to take effect
```

Document this requirement in application setup instructions.

---

## Systemd integration

Bind services to hardware presence. Start a service when a specific device appears; stop it when the device disappears.

### Pattern: service activation on device

```udev
# Rule: 99-radio-monitor.rules
SUBSYSTEM=="tty", ATTRS{idVendor}=="067b", ATTRS{idProduct}=="2303", 
    TAG+="systemd", ENV{SYSTEMD_WANTS}+="radio-daemon@%k.service"
```

```ini
# Service: /etc/systemd/system/radio-daemon@.service
[Unit]
Description=Radio daemon for %i
StopWhenUnneeded=yes

[Service]
Type=simple
ExecStart=/usr/local/bin/radio-daemon /dev/%i
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

### Key systemd udev variables

| Variable | Purpose |
|----------|---------|
| `TAG+="systemd"` | Required. Tells systemd to create a device unit. |
| `ENV{SYSTEMD_WANTS}` | Start this service when device appears. |
| `ENV{SYSTEMD_USER_WANTS}` | Start user service (after logind session). |
| `%k` | Device kernel name (ttyUSB0) in service template. |
| `%n` | Device number (0 for ttyUSB0). |

---

## Testing and debugging

Use `udevadm` to test rules without rebooting or unplugging hardware.

### Test workflow

```bash
# 1. Identify the device path
ls -la /sys/class/tty/ttyUSB0/device/

# 2. Test rule matching (dry run)
sudo udevadm test /sys/class/tty/ttyUSB0 2>&1 | less

# 3. Check current device properties
udevadm info -a -n /dev/ttyUSB0

# 4. Monitor real-time events
sudo udevadm monitor --environment --udev

# 5. Reload rules after editing
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=tty
```

### Debugging checklist

| Problem | Check |
|---------|-------|
| Rule not matching | Use `ATTRS` not `ATTR` for parent USB attributes. |
| Wrong device node permissions | Verify `GROUP="dialout"` and user group membership. |
| Symlink not created | Check for typos in `SYMLINK+=` (note the `+=`). |
| Service not starting | Verify `TAG+="systemd"` and template service syntax. |

---

## Anti-Patterns

1. **`ATTR` instead of `ATTRS` for USB attributes.** `ATTR{idVendor}` on a tty device never matches. USB attributes live on the parent device.

2. **Missing dialout group.** Applications fail with permission denied. Always set `GROUP="dialout"` for serial devices.

3. **Hardcoded device names.** `/dev/ttyUSB0` varies by enumeration order. Use symlinks (via `SYMLINK+=`) or stable paths in `/dev/serial/by-id/`.

4. **`RUN+=` for service management.** Use `ENV{SYSTEMD_WANTS}` instead. `RUN` commands race with systemd; ordering is unreliable.

5. **Numeric prefix > 99.** `100-foo.rules` sorts before `99-local.rules`. Use exactly two digits.

6. **Bare `ACTION=="add"` on child devices.** USB children may already exist when the rule is processed. Match on attributes, not just action.

7. **Overly broad rules.** `SUBSYSTEM=="tty"` without VID/PID matches all serial ports. Be specific to avoid affecting unrelated hardware.

---

## Conventions

1. **Use `99-` prefix** for all site-local rules.
2. **Use `ATTRS` for USB matching.** `ATTR` only checks immediate device.
3. **Always set `GROUP="dialout"`** for serial devices.
4. **Use `SYMLINK+=` for stable aliases.** Never rely on `ttyUSB*` enumeration order.
5. **Use `TAG+="systemd"` for service binding.** Not `RUN+=` scripts.
6. **Test with `udevadm test`** before applying changes.
7. **Reload with `udevadm control --reload-rules`** after edits.
