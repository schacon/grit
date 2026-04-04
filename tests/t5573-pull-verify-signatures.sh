#!/bin/sh
# Ported from git/t/t5573-pull-verify-signatures.sh
# pull signature verification tests

test_description='pull signature verification tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
