#!/bin/sh
#
# Upstream: t9305-fast-import-signatures.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='git fast-import --signed-commits=<mode>'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'set up unsigned initial commit and import repo' '
	false
'

test_expect_failure 'set up OpenPGP signed commit' '
	false
'

test_expect_failure 'import OpenPGP signature with --signed-commits=verbatim' '
	false
'

test_expect_failure 'set up X.509 signed commit' '
	false
'

test_expect_failure 'import X.509 signature fails with --signed-commits=abort' '
	false
'

test_expect_failure 'import X.509 signature with --signed-commits=warn-verbatim' '
	false
'

test_expect_failure 'set up SSH signed commit' '
	false
'

test_expect_failure 'strip SSH signature with --signed-commits=strip' '
	false
'

test_expect_failure 'setup a commit with dual OpenPGP signatures on its SHA-1 and SHA-256 formats' '
	false
'

test_expect_failure 'strip both OpenPGP signatures with --signed-commits=warn-strip' '
	false
'

test_expect_failure 'import commit with no signature with --signed-commits=$mode' '
	false
'

test_expect_failure 'keep valid OpenPGP signature with --signed-commits=$mode' '
	false
'

test_expect_failure 'handle signature invalidated by message change with --signed-commits=$mode' '
	false
'

test_expect_failure 'keep valid X.509 signature with --signed-commits=$mode' '
	false
'

test_expect_failure 'keep valid SSH signature with --signed-commits=$mode' '
	false
'

test_expect_failure 'sign invalid commit with explicit keyid' '
	false
'

test_done
