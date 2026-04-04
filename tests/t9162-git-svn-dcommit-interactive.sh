#!/bin/sh
#
# Upstream: t9162-git-svn-dcommit-interactive.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit --interactive series'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'answers: y [\n] yes' '
	false
'

test_expect_failure 'answers: yes yes no' '
	false
'

test_expect_failure 'answers: yes quit' '
	false
'

test_expect_failure 'answers: all' '
	false
'

test_done
