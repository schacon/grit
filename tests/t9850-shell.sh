#!/bin/sh
#
# Upstream: t9850-shell.sh
# Requires git-shell — ported as test_expect_failure stubs.
#

test_description='git shell tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- git-shell not available in grit ---

test_expect_failure 'shell allows upload-pack' '
	false
'

test_expect_failure 'shell forbids other commands' '
	false
'

test_expect_failure 'shell forbids interactive use by default' '
	false
'

test_expect_failure 'shell allows interactive command' '
	false
'

test_expect_failure 'shell complains of overlong commands' '
	false
'

test_done
