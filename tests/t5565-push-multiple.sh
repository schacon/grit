#!/bin/sh
# Ported from git/t/t5565-push-multiple.sh
# push to group

test_description='push to group'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
