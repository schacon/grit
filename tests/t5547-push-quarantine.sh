#!/bin/sh
# Ported from git/t/t5547-push-quarantine.sh
# check quarantine of objects during push

test_description='check quarantine of objects during push'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
