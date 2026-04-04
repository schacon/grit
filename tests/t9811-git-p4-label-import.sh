#!/bin/sh
#
# Upstream: t9811-git-p4-label-import.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 label tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'basic p4 labels' '
	false
'

test_expect_failure 'two labels on the same changelist' '
	false
'

test_expect_failure 'export git tags to p4' '
	false
'

test_expect_failure 'export git tags to p4 with deletion' '
	false
'

test_expect_failure 'tag that cannot be exported' '
	false
'

test_expect_failure 'use git config to enable import/export of tags' '
	false
'

test_expect_failure 'importing labels with missing revisions' '
	false
'

test_done
