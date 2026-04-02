#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> Extracting tests and running ported tests..."
python3 "$SCRIPT_DIR/subscripts/extract-and-test.py"

echo "==> Generating command status..."
python3 "$SCRIPT_DIR/subscripts/generate-command-status.py"

echo "==> Generating dashboard HTML..."
python3 "$SCRIPT_DIR/subscripts/generate-progress-html.py"

echo "==> Dashboard updated."
