#!/bin/sh
#
# Upstream: t9107-git-svn-migrate.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn metadata migrations from previous versions'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup old-looking metadata' '
	false
'

test_expect_failure 'git-svn-HEAD is a real HEAD' '
	false
'

test_expect_failure 'initialize old-style (v0) git svn layout' '
	false
'

test_expect_failure 'initialize a multi-repository repo' '
	false
'

test_expect_failure 'multi-fetch works on partial urls + paths' '
	false
'

test_expect_failure 'migrate --minimize on old inited layout' '
	false
'

test_expect_failure '.rev_db auto-converted to .rev_map.UUID' '
	false
'

test_done
