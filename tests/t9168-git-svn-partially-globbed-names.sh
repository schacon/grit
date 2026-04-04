#!/bin/sh
#
# Upstream: t9168-git-svn-partially-globbed-names.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn globbing refspecs with prefixed globs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'prepare test refspec prefixed globbing' '
	false
'

test_expect_failure 'test refspec prefixed globbing' '
	false
'

test_expect_failure 'prepare test left-hand-side only prefixed globbing' '
	false
'

test_expect_failure 'test left-hand-side only prefixed globbing' '
	false
'

test_expect_failure 'prepare test prefixed globs match just prefix' '
	false
'

test_expect_failure 'test prefixed globs match just prefix' '
	false
'

test_expect_failure 'prepare test disallow prefixed multi-globs' '
	false
'

test_expect_failure 'test disallow prefixed multi-globs' '
	false
'

test_expect_failure 'prepare test globbing in the middle of the word' '
	false
'

test_expect_failure 'test globbing in the middle of the word' '
	false
'

test_expect_failure 'prepare test disallow multiple asterisks in one word' '
	false
'

test_expect_failure 'test disallow multiple asterisks in one word' '
	false
'

test_done
