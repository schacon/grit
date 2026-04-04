#!/bin/sh
# Ported from git/t/t9306-fast-import-signed-tags.sh
# git fast-import --signed-tags=<mode>

test_description='git fast-import --signed-tags=<mode>'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
