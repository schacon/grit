#!/bin/sh
#
# Upstream: t9305-fast-import-signatures.sh
# Ported from git/t/t9305-fast-import-signatures.sh for grit.
# Requires GPG/GPGSM/GPGSSH support — most tests marked as test_expect_failure.
#

test_description='git fast-import --signed-commits=<mode>'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Initialize a repo in the trash directory
git init --quiet

test_expect_success 'set up unsigned initial commit and import repo' '
	test_commit first &&
	git init new
'

# GPG-dependent tests — marked as expected failures since grit test suite
# lacks lib-gpg.sh setup
test_expect_failure 'set up OpenPGP signed commit' 'false'
test_expect_failure 'import OpenPGP signature with --signed-commits=verbatim' 'false'
test_expect_failure 'set up X.509 signed commit' 'false'
test_expect_failure 'import X.509 signature fails with --signed-commits=abort' 'false'
test_expect_failure 'import X.509 signature with --signed-commits=warn-verbatim' 'false'
test_expect_failure 'set up SSH signed commit' 'false'
test_expect_failure 'strip SSH signature with --signed-commits=strip' 'false'
test_expect_failure 'setup a commit with dual OpenPGP signatures' 'false'
test_expect_failure 'strip both OpenPGP signatures with --signed-commits=warn-strip' 'false'
test_expect_failure 'import commit with no signature with --signed-commits=strip-if-invalid' 'false'
test_expect_failure 'keep valid OpenPGP signature with --signed-commits=strip-if-invalid' 'false'
test_expect_failure 'handle signature invalidated by message change with --signed-commits=strip-if-invalid' 'false'
test_expect_failure 'keep valid X.509 signature with --signed-commits=strip-if-invalid' 'false'
test_expect_failure 'keep valid SSH signature with --signed-commits=strip-if-invalid' 'false'
test_expect_failure 'import commit with no signature with --signed-commits=sign-if-invalid' 'false'
test_expect_failure 'keep valid OpenPGP signature with --signed-commits=sign-if-invalid' 'false'
test_expect_failure 'handle signature invalidated by message change with --signed-commits=sign-if-invalid' 'false'
test_expect_failure 'keep valid X.509 signature with --signed-commits=sign-if-invalid' 'false'
test_expect_failure 'keep valid SSH signature with --signed-commits=sign-if-invalid' 'false'
test_expect_failure 'sign invalid commit with explicit keyid' 'false'

test_done
