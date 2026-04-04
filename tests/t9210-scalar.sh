#!/bin/sh
#
# Upstream: t9210-scalar.sh
# Requires scalar — ported as test_expect_failure stubs.
#

test_description='test the `scalar` command'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- scalar not available in grit ---

test_expect_failure 'scalar shows a usage' '
	false
'

test_expect_failure 'scalar invoked on enlistment root' '
	false
'

test_expect_failure 'scalar invoked on enlistment src repo' '
	false
'

test_expect_failure 'scalar invoked when enlistment root and repo are the same' '
	false
'

test_expect_failure 'scalar repo search respects GIT_CEILING_DIRECTORIES' '
	false
'

test_expect_failure 'scalar enlistments need a worktree' '
	false
'

test_expect_failure 'scalar register starts fsmon daemon' '
	false
'

test_expect_failure 'scalar register warns when background maintenance fails' '
	false
'

test_expect_failure 'scalar unregister' '
	false
'

test_expect_failure 'scalar register --no-maintenance' '
	false
'

test_expect_failure 'set up repository to clone' '
	false
'

test_expect_failure 'scalar clone' '
	false
'

test_expect_failure 'scalar clone --no-... opts' '
	false
'

test_expect_failure 'scalar reconfigure' '
	false
'

test_expect_failure 'scalar reconfigure --all with includeIf.onbranch' '
	false
'

test_expect_failure 'scalar reconfigure --all with detached HEADs' '
	false
'

test_expect_failure '`reconfigure -a` removes stale config entries' '
	false
'

test_expect_failure 'scalar delete without enlistment shows a usage' '
	false
'

test_expect_failure 'scalar delete with enlistment' '
	false
'

test_expect_failure 'scalar supports -c/-C' '
	false
'

test_expect_failure '`scalar [...] <dir>` errors out when dir is missing' '
	false
'

test_expect_failure 'scalar diagnose' '
	false
'

test_done
