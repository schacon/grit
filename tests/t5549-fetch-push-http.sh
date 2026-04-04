#!/bin/sh
# Ported from git/t/t5549-fetch-push-http.sh
# fetch/push functionality using the HTTP protocol

test_description='fetch/push functionality using the HTTP protocol'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
