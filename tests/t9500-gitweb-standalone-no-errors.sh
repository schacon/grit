#!/bin/sh
# Ported from git/t/t9500-gitweb-standalone-no-errors.sh
# gitweb as standalone script (basic tests).

test_description='gitweb as standalone script (basic tests).'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'gitweb (requires CGI) — not yet ported' '
	false
'

test_done
