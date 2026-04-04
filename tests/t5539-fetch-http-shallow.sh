#!/bin/sh
# Ported from git/t/t5539-fetch-http-shallow.sh
# fetch/clone from a shallow clone over http

test_description='fetch/clone from a shallow clone over http'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
