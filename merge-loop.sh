#!/bin/bash

while true; do
  echo "Getting next PR"

  PR_COUNT=$(gh search prs --repo schacon/grit --state=open --draft=false --limit 1000 --json number --jq 'length')
  echo "${PR_COUNT} Pull Requests Left"

  COMMIT=$(git rev-parse --short=6 HEAD)
  LOGFILE="logs/agent_${COMMIT}.log"

  agent --yolo \
    -p "$(cat MERGE_PROMPT.md)" \
    --model composer-2-fast &>"$LOGFILE"

  cat $LOGFILE
done
