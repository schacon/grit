#!/bin/sh
#
# Upstream: t9817-git-p4-exclude.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 tests for excluded paths during clone and sync'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'create exclude repo' '
	false
'

test_expect_failure 'check the repo was created correctly' '
	false
'

test_expect_failure 'clone, excluding part of repo' '
	false
'

test_expect_failure 'clone, excluding single file, no trailing /' '
	false
'

test_expect_failure 'clone, then sync with exclude' '
	false
'

test_expect_failure 'clone, then sync with exclude, no trailing /' '
	false
'

test_done
