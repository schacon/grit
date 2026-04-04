#!/bin/sh
# Ported from git/t/t5580-unc-paths.sh
# various Windows-only path tests

test_description='various Windows-only path tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
