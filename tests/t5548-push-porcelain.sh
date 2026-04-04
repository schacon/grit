#!/bin/sh
# Ported from git/t/t5548-push-porcelain.sh
# Test git push porcelain output

test_description='Test git push porcelain output'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
