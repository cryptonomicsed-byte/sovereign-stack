# ── Stage 1: Build React/Vite frontend ───────────────────────────────────────
FROM node:20-alpine AS frontend-builder

WORKDIR /build/frontend
COPY frontend/package.json ./
RUN npm install

COPY frontend/ ./
RUN npm run build
# Output: /build/frontend/dist/

# ── Stage 2: Python backend + static serving ─────────────────────────────────
FROM python:3.11-slim AS runtime

WORKDIR /app

# System deps (nginx for static serving + curl for healthcheck)
RUN apt-get update && apt-get install -y --no-install-recommends \
    nginx curl \
    && rm -rf /var/lib/apt/lists/*

# Python deps
COPY backend/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# App code + data
COPY backend/ ./backend/
COPY data/    ./data/
COPY workers/ ./workers/

# Copy built frontend into nginx's docroot
COPY --from=frontend-builder /build/frontend/dist /var/www/veilsim

# nginx config: serve frontend on :3000, proxy /api → FastAPI :8788
RUN cat > /etc/nginx/sites-available/default <<'EOF'
server {
    listen 3000 default_server;
    root /var/www/veilsim;
    index index.html;

    # Proxy API calls to FastAPI backend
    location /api/ {
        proxy_pass         http://127.0.0.1:8788/;
        proxy_http_version 1.1;
        proxy_set_header   Host $host;
        proxy_set_header   X-Real-IP $remote_addr;
    }

    # SPA fallback
    location / {
        try_files $uri $uri/ /index.html;
    }
}
EOF

COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 8788 3000

ENTRYPOINT ["/entrypoint.sh"]
