#!/bin/sh
# Ported from git/t/t5551-http-fetch-smart.sh
# test smart fetching over http via http-backend ($HTTP_PROTO)

test_description='test smart fetching over http via http-backend ($HTTP_PROTO)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
