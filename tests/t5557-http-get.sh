#!/bin/sh
# Ported from git/t/t5557-http-get.sh
# test downloading a file by URL

test_description='test downloading a file by URL'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
