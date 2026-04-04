#!/bin/sh
#
# Upstream: t9124-git-svn-dcommit-auto-props.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn dcommit honors auto-props'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize git svn' '
	false
'

test_expect_failure 'enable auto-props config' '
	false
'

test_expect_failure 'add files matching auto-props' '
	false
'

test_expect_failure 'disable auto-props config' '
	false
'

test_expect_failure 'add files matching disabled auto-props' '
	false
'

test_expect_failure 'check resulting svn repository' '
	false
'

test_expect_failure 'check renamed file' '
	false
'

test_done
