#!/bin/sh
#
# Upstream: t9601-cvsimport-vendor-branch.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport handling of vendor branches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'import a module with a vendor branch' '
	false
'

test_expect_failure 'check HEAD out of cvs repository' '
	false
'

test_expect_failure 'check main out of git repository' '
	false
'

test_expect_failure 'check a file that was imported once' '
	false
'

test_expect_failure 'check a file that was imported twice' '
	false
'

test_expect_failure 'check a file that was imported then modified on HEAD' '
	false
'

test_expect_failure 'check a file that was imported, modified, then imported again' '
	false
'

test_expect_failure 'check a file that was added to HEAD then imported' '
	false
'

test_expect_failure 'a vendor branch whose tag has been removed' '
	false
'

test_done
