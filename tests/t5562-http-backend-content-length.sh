#!/bin/sh
# Ported from git/t/t5562-http-backend-content-length.sh
# test git-http-backend respects CONTENT_LENGTH

test_description='test git-http-backend respects CONTENT_LENGTH'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
