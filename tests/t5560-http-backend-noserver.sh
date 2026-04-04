#!/bin/sh
# Ported from git/t/t5560-http-backend-noserver.sh
# test git-http-backend-noserver

test_description='test git-http-backend-noserver'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
