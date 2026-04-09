#!/bin/bash

while true; do
  COMMIT=$(git rev-parse --short=6 HEAD)
  LOGFILE="logs/agent_${COMMIT}.log"

  claude --dangerously-skip-permissions \
    -p "$(cat MERGE_PROMPT.md)" \
    --model claude-opus-X-Y &>"$LOGFILE"
done
