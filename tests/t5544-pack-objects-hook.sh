#!/bin/sh
# Ported from git/t/t5544-pack-objects-hook.sh
# test custom script in place of pack-objects

test_description='test custom script in place of pack-objects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
