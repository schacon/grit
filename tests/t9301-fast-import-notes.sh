#!/bin/sh
# Ported from git/t/t9301-fast-import-notes.sh
# test git fast-import of notes objects

test_description='test git fast-import of notes objects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
