#!/bin/sh
#
# Upstream: t9602-cvsimport-branches-tags.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport handling of branches and tags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'import module' '
	false
'

test_expect_failure 'test branch main' '
	false
'

test_expect_failure 'test branch vendorbranch' '
	false
'

test_expect_failure 'test branch B_FROM_INITIALS' '
	false
'

test_expect_failure 'test branch B_FROM_INITIALS_BUT_ONE' '
	false
'

test_expect_failure 'test branch B_MIXED' '
	false
'

test_expect_failure 'test branch B_SPLIT' '
	false
'

test_expect_failure 'test tag vendortag' '
	false
'

test_expect_failure 'test tag T_ALL_INITIAL_FILES' '
	false
'

test_expect_failure 'test tag T_ALL_INITIAL_FILES_BUT_ONE' '
	false
'

test_expect_failure 'test tag T_MIXED' '
	false
'

test_done
