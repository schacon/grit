#!/bin/sh
#
# Upstream: t9132-git-svn-broken-symlink.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='test that git handles an svn repository with empty symlinks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn dumpfile' '
	false
'

test_expect_failure 'clone using git svn' '
	false
'

test_expect_failure '"bar" is a symlink that points to "asdf"' '
	false
'

test_expect_failure 'get "bar" => symlink fix from svn' '
	false
'

test_expect_failure '"bar" remains a proper symlink' '
	false
'

test_done
