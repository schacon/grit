#!/bin/sh
#
# Upstream: t9104-git-svn-follow-parent.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn fetching'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'init and fetch a moved directory' '
	false
'

test_expect_failure 'init and fetch from one svn-remote' '
	false
'

test_expect_failure 'follow deleted parent' '
	false
'

test_expect_failure 'follow larger parent' '
	false
'

test_expect_failure 'follow higher-level parent' '
	false
'

test_expect_failure 'follow deleted directory' '
	false
'

test_expect_failure 'follow-parent avoids deleting relevant info' '
	false
'

test_expect_failure 'track initial change if it was only made to parent' '
	false
'

test_expect_failure 'follow-parent is atomic' '
	false
'

test_expect_failure 'track multi-parent paths' '
	false
'

test_expect_failure 'multi-fetch continues to work' '
	false
'

test_expect_failure 'multi-fetch works off a '\''clean'\'' repository' '
	false
'

test_done
