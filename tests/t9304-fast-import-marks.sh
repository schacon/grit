#!/bin/sh
#
# Upstream: t9304-fast-import-marks.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='test exotic situations with marks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'setup dump of basic history' '
	false
'

test_expect_failure 'setup large marks file' '
	false
'

test_expect_failure 'import with large marks file' '
	false
'

test_expect_failure 'setup dump with submodule' '
	false
'

test_expect_failure 'setup submodule mapping with large id' '
	false
'

test_expect_failure 'import with submodule mapping' '
	false
'

test_expect_failure 'paths adjusted for relative subdir' '
	false
'

test_expect_failure 'relative marks are not affected by subdir' '
	false
'

test_done
