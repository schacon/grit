#!/bin/sh
#
# Upstream: t9129-git-svn-i18n-commitencoding.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn honors i18n.commitEncoding in config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure '$H setup' '
	false
'

test_expect_failure '$H commit on git side' '
	false
'

test_expect_failure '$H dcommit to svn' '
	false
'

test_done
