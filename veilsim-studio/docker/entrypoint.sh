#!/bin/sh
# Start nginx (frontend) and FastAPI backend in the same container.
# For production, split into separate containers via docker-compose.

set -e

# Start nginx in background
nginx -g "daemon off;" &

# Start FastAPI backend
cd /app
exec uvicorn backend.main:app \
    --host 0.0.0.0 \
    --port 8788 \
    --workers 2
