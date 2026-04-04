#!/bin/sh
# Ported from git/t/t5581-http-curl-verbose.sh
# test GIT_CURL_VERBOSE

test_description='test GIT_CURL_VERBOSE'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
