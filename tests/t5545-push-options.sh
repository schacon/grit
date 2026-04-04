#!/bin/sh
# Ported from git/t/t5545-push-options.sh
# pushing to a repository using push options

test_description='pushing to a repository using push options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
