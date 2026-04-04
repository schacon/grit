#!/bin/sh
#
# Upstream: t9801-git-p4-branch.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 tests for p4 branches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'basic p4 branches' '
	false
'

test_expect_failure 'import main, no branch detection' '
	false
'

test_expect_failure 'import branch1, no branch detection' '
	false
'

test_expect_failure 'import branch2, no branch detection' '
	false
'

test_expect_failure 'import depot, no branch detection' '
	false
'

test_expect_failure 'import depot, branch detection' '
	false
'

test_expect_failure 'sync specific detected branch' '
	false
'

test_expect_failure 'import depot, branch detection, branchList branch definition' '
	false
'

test_expect_failure 'restart p4d' '
	false
'

test_expect_failure 'add simple p4 branches' '
	false
'

test_expect_failure 'git p4 clone simple branches' '
	false
'

test_expect_failure 'git p4 add complex branches' '
	false
'

test_expect_failure 'git p4 clone complex branches' '
	false
'

test_expect_failure 'git p4 submit to two branches in a single changelist' '
	false
'

test_expect_failure 'git p4 sync changes to two branches in the same changelist' '
	false
'

test_expect_failure 'git p4 file subset branch' '
	false
'

test_expect_failure 'git p4 clone file subset branch' '
	false
'

test_expect_failure 'git p4 clone complex branches with excluded files' '
	false
'

test_expect_failure 'use-client-spec detect-branches setup' '
	false
'

test_expect_failure 'use-client-spec detect-branches files in top-level' '
	false
'

test_expect_failure 'use-client-spec detect-branches skips branches setup' '
	false
'

test_expect_failure 'use-client-spec detect-branches skips branches' '
	false
'

test_expect_failure 'use-client-spec detect-branches skips files in branches' '
	false
'

test_expect_failure 'restart p4d' '
	false
'

test_expect_failure 'add simple p4 branches with common base folder on each branch' '
	false
'

test_expect_failure 'git p4 clone simple branches with base folder on server side' '
	false
'

test_expect_failure 'Update a file in git side and submit to P4 using client view' '
	false
'

test_expect_failure 'restart p4d (case folding enabled)' '
	false
'

test_expect_failure 'basic p4 branches for case folding' '
	false
'

test_expect_failure 'git p4 clone, branchList branch definition, ignorecase' '
	false
'

test_expect_failure 'git p4 clone with client-spec, branchList branch definition, ignorecase' '
	false
'

test_done
