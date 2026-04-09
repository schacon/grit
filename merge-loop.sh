#!/bin/bash

while true; do
  echo "Getting next PR"
  COMMIT=$(git rev-parse --short=6 HEAD)
  LOGFILE="logs/agent_${COMMIT}.log"

  agent --yolo \
    -p "$(cat MERGE_PROMPT.md)" \
    --model composer-2-fast &>"$LOGFILE"
done
