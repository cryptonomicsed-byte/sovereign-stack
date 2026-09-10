FROM python:3.12-slim AS base

WORKDIR /app

# System deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl wget ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Python deps
COPY backend/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# App code + data
COPY backend/ ./backend/
COPY data/    ./data/

EXPOSE 8788

CMD ["uvicorn", "backend.main:app", "--host", "0.0.0.0", "--port", "8788", "--workers", "2"]
