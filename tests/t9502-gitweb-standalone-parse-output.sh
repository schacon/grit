#!/bin/sh
# Ported from git/t/t9502-gitweb-standalone-parse-output.sh
# gitweb as standalone script (parsing script output).

test_description='gitweb as standalone script (parsing script output).'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'gitweb (requires CGI) — not yet ported' '
	false
'

test_done
