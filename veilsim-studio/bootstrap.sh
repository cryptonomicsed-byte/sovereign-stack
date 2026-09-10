#!/usr/bin/env bash
# VeilSim Studio bootstrap — starts the ỌSỌVM execution endpoint.
# sovereign-node points OsovmEngine::with_endpoint("http://localhost:8788") here.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== VeilSim Studio bootstrap ==="
echo "ỌSỌVM endpoint will be available at http://localhost:8788"

# Check Python
if ! command -v python3 &>/dev/null; then
  echo "ERROR: python3 not found" && exit 1
fi

# Install deps if needed
if ! python3 -c "import fastapi" 2>/dev/null; then
  echo "Installing Python dependencies..."
  pip install -r backend/requirements.txt
fi

# Start the API server
echo "Starting VeilSim Studio API..."
cd "$SCRIPT_DIR" && python3 -m uvicorn backend.main:app \
  --host 0.0.0.0 \
  --port 8788 \
  --reload \
  --log-level info
