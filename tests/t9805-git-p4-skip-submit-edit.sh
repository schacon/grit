#!/bin/sh
#
# Upstream: t9805-git-p4-skip-submit-edit.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 skipSubmitEdit config variables'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'no config, unedited, say yes' '
	false
'

test_expect_failure 'no config, unedited, say no' '
	false
'

test_expect_failure 'skipSubmitEdit' '
	false
'

test_expect_failure 'skipSubmitEditCheck' '
	false
'

test_expect_failure 'no config, edited' '
	false
'

test_done
