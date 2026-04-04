#!/bin/sh
# Ported from git/t/t5582-fetch-negative-refspec.sh
# fetch-negative-refspec

test_description='fetch-negative-refspec'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
