#!/bin/sh
# Ported from git/t/t5550-http-fetch-dumb.sh
# test dumb fetching over http via static file

test_description='test dumb fetching over http via static file'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
