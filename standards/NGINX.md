# NGINX

> Additive to STANDARDS.md. Read that first. Everything here is NGINX-specific.
>
> Covers: NGINX configuration, reverse proxy patterns, SSL/TLS, security headers, rate limiting, and load balancing.
>
> **Key decisions:** explicit `server` blocks, no default server, security headers mandatory, rate limiting by default, SSL labs A+ target.

---

## Configuration structure

### File organization

| File | Purpose |
|------|---------|
| `nginx.conf` | Main configuration, global settings, includes |
| `conf.d/*.conf` | Site-specific configurations (one per service) |
| `sites-available/` | Complete server block definitions |
| `sites-enabled/` | Symlinks to activated sites |

```nginx
# nginx.conf - minimal, delegates to conf.d/
user nginx;
worker_processes auto;
error_log /var/log/nginx/error.log warn;
pid /run/nginx.pid;

events {
    worker_connections 4096;
    use epoll;
    multi_accept on;
}

http {
    include /etc/nginx/mime.types;
    default_type application/octet-stream;

    # Logging format
    log_format main '$remote_addr - $remote_user [$time_local] "$request" '
                    '$status $body_bytes_sent "$http_referer" '
                    '"$http_user_agent" $request_time';

    access_log /var/log/nginx/access.log main;

    # Performance
    sendfile on;
    tcp_nopush on;
    tcp_nodelay on;
    keepalive_timeout 65;

    # Security defaults
    include /etc/nginx/conf.d/security.conf;

    # Site configurations
    include /etc/nginx/sites-enabled/*;
}
```

### Security baseline (conf.d/security.conf)

```nginx
# Hide NGINX version
server_tokens off;

# Security headers
add_header X-Frame-Options "SAMEORIGIN" always;
add_header X-Content-Type-Options "nosniff" always;
add_header X-XSS-Protection "1; mode=block" always;
add_header Referrer-Policy "strict-origin-when-cross-origin" always;

# Permissions Policy (formerly Feature-Policy)
add_header Permissions-Policy "geolocation=(), microphone=(), camera=()" always;

# Size limits
client_max_body_size 10m;
client_body_buffer_size 16k;
client_header_buffer_size 1k;
large_client_header_buffers 4 8k;

# Timeouts
client_body_timeout 12;
client_header_timeout 12;
keepalive_timeout 15;

# Rate limiting zone (defined here, applied per-server)
limit_req_zone $binary_remote_addr zone=general:10m rate=10r/s;
limit_conn_zone $binary_remote_addr zone=addr:10m;
```

---

## Server blocks

### Reverse proxy template

```nginx
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name api.example.com;

    # SSL configuration
    ssl_certificate /etc/ssl/certs/api.example.com.crt;
    ssl_certificate_key /etc/ssl/private/api.example.com.key;
    ssl_session_timeout 1d;
    ssl_session_cache shared:SSL:50m;
    ssl_session_tickets off;

    # Modern TLS only
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;

    # HSTS (uncomment only when HTTPS is fully working)
    # add_header Strict-Transport-Security "max-age=63072000" always;

    # Rate limiting
    limit_req zone=general burst=20 nodelay;
    limit_conn addr 10;

    # Proxy to upstream
    location / {
        proxy_pass http://localhost:8080;
        proxy_http_version 1.1;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Connection "";

        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }

    # Health check endpoint (bypass rate limiting if needed)
    location /health {
        proxy_pass http://localhost:8080/health;
        proxy_http_version 1.1;
        proxy_set_header Host $host;

        # Optional: exempt from rate limiting
        limit_req off;
    }
}
```

### Static file serving

```nginx
server {
    listen 443 ssl http2;
    server_name static.example.com;

    root /var/www/static;
    index index.html;

    # Cache static assets
    location ~* \.(js|css|png|jpg|jpeg|gif|ico|svg|woff|woff2)$ {
        expires 6m;
        access_log off;
        add_header Cache-Control "public, immutable";
    }

    # Security: deny access to hidden files
    location ~ /\. {
        deny all;
        access_log off;
        log_not_found off;
    }

    # Security: deny access to backup/config files
    location ~* \.(bak|config|sql|fla|psd|ini|log|sh|inc|swp|dist)$ {
        deny all;
        access_log off;
        log_not_found off;
    }
}
```

---

## SSL/TLS

### Certificate handling

- Store certificates in `/etc/ssl/certs/` (public) and `/etc/ssl/private/` (private)
- Private key files must be `chmod 600`, owned by root
- Use Let's Encrypt for public-facing services (automated renewal)
- Test configuration with: `nginx -t && systemctl reload nginx`

### SSL Labs A+ configuration

```nginx
server {
    listen 443 ssl http2;

    ssl_certificate /path/to/fullchain.pem;
    ssl_certificate_key /path/to/privkey.pem;

    # Session settings
    ssl_session_timeout 1d;
    ssl_session_cache shared:SSL:50m;
    ssl_session_tickets off;

    # Protocols and ciphers (Mozilla Intermediate, 2024)
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;

    # OCSP Stapling
    ssl_stapling on;
    ssl_stapling_verify on;
    ssl_trusted_certificate /path/to/chain.pem;
    resolver 1.1.1.1 8.8.8.8 valid=300s;
    resolver_timeout 5s;

    # HSTS
    add_header Strict-Transport-Security "max-age=63072000" always;
}
```

### HTTP to HTTPS redirect

```nginx
# Redirect all HTTP to HTTPS
server {
    listen 80;
    listen [::]:80;
    server_name _;

    location / {
        return 301 https://$host$request_uri;
    }

    # Let's Encrypt challenge (certbot)
    location /.well-known/acme-challenge/ {
        root /var/www/certbot;
    }
}
```

---

## Rate limiting

### Zone definitions

```nginx
# Per-IP rate limiting
limit_req_zone $binary_remote_addr zone=ip:10m rate=10r/s;

# Per-user rate limiting (requires auth)
limit_req_zone $http_authorization zone=user:10m rate=30r/s;

# Per-API-key rate limiting
map $http_x_api_key $api_key_limit {
    "" $binary_remote_addr;
    default $http_x_api_key;
}
limit_req_zone $api_key_limit zone=api_key:10m rate=100r/s;
```

### Application

```nginx
server {
    location /api/ {
        # Burst allows short spikes, nodelay processes immediately if bucket has tokens
        limit_req zone=api_key burst=50 nodelay;

        # Return 429 instead of 503 when rate limited
        limit_req_status 429;

        proxy_pass http://backend;
    }
}
```

---

## Load balancing

### Upstream definitions

```nginx
upstream backend {
    least_conn;  # or ip_hash, least_time (NGINX Plus)

    server 10.0.0.1:8080 weight=5;
    server 10.0.0.2:8080 weight=5;
    server 10.0.0.3:8080 backup;

    keepalive 32;
    keepalive_timeout 60s;
    keepalive_requests 1000;
}
```

### Health checks (active)

```nginx
upstream backend {
    zone upstream_backend 64k;

    server 10.0.0.1:8080;
    server 10.0.0.2:8080;

    # NGINX Plus only
    # health_check interval=5s fails=3 passes=2 uri=/health;
}

server {
    location / {
        proxy_pass http://backend;
        health_check;  # NGINX Plus
    }
}
```

### Passive health checks (open source)

```nginx
upstream backend {
    server 10.0.0.1:8080 max_fails=3 fail_timeout=30s;
    server 10.0.0.2:8080 max_fails=3 fail_timeout=30s;
}
```

---

## Security

### Access control

```nginx
# IP-based restriction
location /admin {
    allow 10.0.0.0/8;
    allow 192.168.0.0/16;
    deny all;

    proxy_pass http://backend;
}

# Basic auth (fallback only, not for primary auth)
location /status {
    auth_basic "Restricted";
    auth_basic_user_file /etc/nginx/.htpasswd;

    proxy_pass http://backend;
}
```

### Request filtering

```nginx
# Block common attack patterns
location / {
    # Block common exploits
    if ($request_uri ~* "(eval\(|base64_decode|\.\.\/|union.*select.*\()") {
        return 403;
    }

    # Block specific user agents
    if ($http_user_agent ~* (wget|curl|nikto|sqlmap|nmap|masscan)) {
        return 403;
    }

    proxy_pass http://backend;
}
```

### WebSocket support

```nginx
location /ws {
    proxy_pass http://backend;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 86400;
}
```

---

## Logging

### Structured logging (JSON)

```nginx
log_format json_analytics escape=json '{"time":"$time_iso8601",'
    '"remote_addr":"$remote_addr",'
    '"request":"$request",'
    '"status":$status,'
    '"bytes_sent":$bytes_sent,'
    '"request_time":$request_time,'
    '"upstream_time":"$upstream_response_time",'
    '"user_agent":"$http_user_agent"}';

access_log /var/log/nginx/access.log json_analytics;
```

### Conditional logging

```nginx
# Don't log health checks
map $request_uri $loggable {
    /health 0;
    default 1;
}

access_log /var/log/nginx/access.log main if=$loggable;
```

---

## Validation

### Pre-deployment checks

```bash
# Syntax validation
nginx -t

# Test specific configuration file
nginx -t -c /path/to/nginx.conf

# Check for common misconfigurations
grep -E "(listen.*80[^0-9]|ssl_certificate.*selfsigned)" /etc/nginx/sites-enabled/*
```

### Reload vs restart

```bash
# Reload configuration without dropping connections
systemctl reload nginx

# Or directly
nginx -s reload
```

---

## Anti-patterns

| Anti-pattern | Problem | Fix |
|-------------|---------|-----|
| `if` in location blocks | Unpredictable behavior, often breaks | Use `map` or separate locations |
| `server_name _;` as default | Vague, may expose unintended content | Explicit default server with 444 or 404 |
| `ssl_protocols TLSv1 TLSv1.1;` | Deprecated, insecure protocols | TLSv1.2+ only |
| No rate limiting | Vulnerable to abuse | Add `limit_req` to all public endpoints |
| `proxy_pass` without headers | Backend sees wrong client IP | Always set `X-Forwarded-*` headers |
| Root inside location block | Confusing inheritance | Set `root` at server level |
| `access_log off` globally | No visibility | Keep access logs, use conditional logging |
| Missing `server_tokens off;` | Version disclosure in errors | Always disable |
| HSTS before HTTPS works | Site becomes unreachable | Enable only after HTTPS is verified |
| `client_max_body_size` default | 1MB too small for APIs | Increase explicitly per use case |

---

## Tooling

| Tool | Purpose |
|------|---------|
| `nginx -t` | Configuration syntax test |
| `nginx -s reload` | Graceful configuration reload |
| `nginx -V` | Show compiled modules and flags |
| `curl -I` | Test headers and response codes |
| `openssl s_client -connect` | Test SSL/TLS configuration |
| [SSL Labs Test](https://www.ssllabs.com/ssltest/) | External SSL assessment |
| `certbot` | Let's Encrypt certificate automation |
| `goaccess` | Log analyzer with real-time dashboard |

---

## Cross-references

| Topic | Standard |
|-------|----------|
| Security headers | SECURITY.md#headers |
| SSL/TLS configuration | SECURITY.md#transport |
| Rate limiting strategy | SECURITY.md#rate-limiting |
| Log aggregation | OPERATIONS.md#observability |
| Health checks | OPERATIONS.md#monitoring |
