#!/bin/sh
#
# Upstream: t9806-git-p4-options.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'clone no --git-dir' '
	false
'

test_expect_failure 'clone --branch should checkout main' '
	false
'

test_expect_failure 'sync when no master branch prints a nice error' '
	false
'

test_expect_failure 'sync --branch builds the full ref name correctly' '
	false
'

test_expect_failure 'sync when two branches but no master should noop' '
	false
'

test_expect_failure 'sync --branch updates specific branch, no detection' '
	false
'

test_expect_failure 'clone creates HEAD symbolic reference' '
	false
'

test_expect_failure 'clone --branch creates HEAD symbolic reference' '
	false
'

test_expect_failure 'clone --changesfile' '
	false
'

test_expect_failure 'clone --changesfile, @all' '
	false
'

test_expect_failure 'clone/sync --import-local' '
	false
'

test_expect_failure 'clone --max-changes' '
	false
'

test_expect_failure 'clone --keep-path' '
	false
'

test_expect_failure 'clone --use-client-spec' '
	false
'

test_expect_failure 'submit works with no p4/master' '
	false
'

test_expect_failure 'submit works with two branches' '
	false
'

test_expect_failure 'use --git-dir option and GIT_DIR' '
	false
'

test_done
