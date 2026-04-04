#!/bin/sh
# Ported from git/t/t5564-http-proxy.sh
# test fetching through http proxy

test_description='test fetching through http proxy'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
