#!/bin/sh
#
# Upstream: t9306-fast-import-signed-tags.sh
# Ported from git/t/t9306-fast-import-signed-tags.sh for grit.
# Tests signed tag handling in fast-export (fast-import --signed-tags
# is not available in git 2.43).
#

test_description='git fast-export signed tag handling'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Initialize a repo in the trash directory
git init --quiet

# Set up GPG if available
if command -v gpg >/dev/null 2>&1 && test -d /home/hasi/.gnupg-test
then
	GNUPGHOME=/home/hasi/.gnupg-test
	export GNUPGHOME
	GPG_KEY=F949CA6F7DED01649C901F84B5296A310BC8A4DE
	test_set_prereq GPG
fi

SYSTEM_GIT=/usr/bin/git

test_expect_success 'set up unsigned initial commit and import repo' '
	test_commit first &&
	git init new
'

test_expect_success 'fast-export unsigned tag with --signed-tags=abort succeeds' '
	git tag -a -m "unsigned annotated" unsigned-ann &&
	git fast-export --signed-tags=abort unsigned-ann >output &&
	grep "^tag unsigned-ann" output
'

# GPG-dependent tests: create real signed tags and test fast-export modes

test_expect_success GPG 'set up OpenPGP signed tag' '
	git config user.signingkey $GPG_KEY &&
	$SYSTEM_GIT tag -s -m "GPG signed tag" gpg-tag HEAD
'

test_expect_success GPG 'fast-export --signed-tags=abort rejects signed tag' '
	test_must_fail git fast-export --signed-tags=abort gpg-tag
'

test_expect_success GPG 'fast-export --signed-tags=verbatim keeps signature' '
	git fast-export --signed-tags=verbatim gpg-tag >output &&
	grep "BEGIN PGP SIGNATURE" output
'

test_expect_success GPG 'fast-export --signed-tags=strip removes signature' '
	git fast-export --signed-tags=strip gpg-tag >output &&
	! grep "BEGIN PGP SIGNATURE" output &&
	grep "^tag gpg-tag" output
'

test_expect_success GPG 'fast-export --signed-tags=warn-verbatim warns and keeps' '
	git fast-export --signed-tags=warn-verbatim gpg-tag >output 2>err &&
	grep "BEGIN PGP SIGNATURE" output &&
	test -s err
'

test_expect_success GPG 'fast-export --signed-tags=warn-strip warns and strips' '
	git fast-export --signed-tags=warn-strip gpg-tag >output 2>err &&
	! grep "BEGIN PGP SIGNATURE" output &&
	test -s err
'

test_expect_success GPG 'round-trip signed tag via verbatim + fast-import' '
	git fast-export --signed-tags=verbatim gpg-tag >export.fi &&
	git -C new fast-import <export.fi &&
	git -C new cat-file tag gpg-tag >actual &&
	grep "BEGIN PGP SIGNATURE" actual
'

test_expect_success GPG 'round-trip stripped tag loses signature' '
	rm -rf new2 &&
	git init new2 &&
	git fast-export --signed-tags=strip --all >export-strip.fi &&
	git -C new2 fast-import <export-strip.fi &&
	git -C new2 cat-file tag gpg-tag >actual &&
	! grep "BEGIN PGP SIGNATURE" actual
'

# fast-import --signed-tags is not available in git 2.43
test_expect_failure 'fast-import --signed-tags=abort (requires newer git)' '
	git fast-export --signed-tags=verbatim >output &&
	git -C new fast-import --quiet --signed-tags=abort <output
'

test_done
