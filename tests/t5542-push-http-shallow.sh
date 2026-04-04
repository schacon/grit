#!/bin/sh
# Ported from git/t/t5542-push-http-shallow.sh
# push from/to a shallow clone over http

test_description='push from/to a shallow clone over http'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
