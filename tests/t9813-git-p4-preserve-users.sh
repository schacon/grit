#!/bin/sh
#
# Upstream: t9813-git-p4-preserve-users.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 preserve users'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'create files' '
	false
'

test_expect_failure 'preserve users' '
	false
'

test_expect_failure 'refuse to preserve users without perms' '
	false
'

test_expect_failure 'preserve user where author is unknown to p4' '
	false
'

test_expect_failure 'not preserving user with mixed authorship' '
	false
'

test_done
