#!/bin/sh
#
# Upstream: t9306-fast-import-signed-tags.sh
# Ported from git/t/t9306-fast-import-signed-tags.sh for grit.
# Requires GPG/GPGSM/GPGSSH — most tests marked as test_expect_failure.
#

test_description='git fast-import --signed-tags=<mode>'

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

test_expect_failure 'import no signed tag with --signed-tags=abort' '
	git fast-export --signed-tags=verbatim >output &&
	git -C new fast-import --quiet --signed-tags=abort <output
'

# GPG-dependent tests
test_expect_failure 'set up OpenPGP signed tag' 'false'
test_expect_failure 'import OpenPGP signed tag with --signed-tags=abort' 'false'
test_expect_failure 'import OpenPGP signed tag with --signed-tags=verbatim' 'false'
test_expect_failure 'setup X.509 signed tag' 'false'
test_expect_failure 'import X.509 signed tag with --signed-tags=warn-strip' 'false'
test_expect_failure 'setup SSH signed tag' 'false'
test_expect_failure 'import SSH signed tag with --signed-tags=warn-verbatim' 'false'
test_expect_failure 'import SSH signed tag with --signed-tags=strip' 'false'

test_done
