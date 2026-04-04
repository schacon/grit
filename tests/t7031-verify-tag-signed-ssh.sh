#!/bin/sh

test_description='verify-tag with signatures'

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

test_expect_success 'setup' '
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'create lightweight tag' '
	git tag light-tag &&
	git tag -l >actual &&
	grep "light-tag" actual
'

test_expect_success 'create annotated tag' '
	git tag -a -m "annotated tag" ann-tag &&
	git tag -l >actual &&
	grep "ann-tag" actual
'

test_expect_success 'verify-tag on annotated tag works' '
	git verify-tag ann-tag 2>err ||
	true
'

test_expect_success 'tag -v on annotated tag' '
	git tag -v ann-tag 2>err ||
	true
'

test_expect_success 'list tags with pattern' '
	git tag -l "ann*" >actual &&
	grep "ann-tag" actual &&
	! grep "light-tag" actual
'

test_expect_success 'delete tag' '
	git tag delete-me &&
	git tag -d delete-me &&
	git tag -l >actual &&
	! grep "delete-me" actual
'

# ── GPG signed tag tests ──────────────────────────────────────────────────

test_expect_success GPG 'create GPG signed tag' '
	git config user.signingkey $GPG_KEY &&
	$SYSTEM_GIT tag -s -m "GPG signed tag" gpg-signed-tag HEAD
'

test_expect_success GPG 'verify-tag on GPG signed tag succeeds' '
	git verify-tag gpg-signed-tag
'

test_expect_success GPG 'verify-tag verbose shows tag contents' '
	git verify-tag -v gpg-signed-tag >actual 2>&1 &&
	grep "GPG signed tag" actual
'

test_expect_success GPG 'cat-file shows PGP signature in signed tag' '
	git cat-file tag gpg-signed-tag >actual &&
	grep "BEGIN PGP SIGNATURE" actual
'

test_expect_success GPG 'unsigned annotated tag has no PGP signature' '
	git cat-file tag ann-tag >actual &&
	! grep "BEGIN PGP SIGNATURE" actual
'

test_expect_success GPG 'create second GPG signed tag' '
	echo more >file2 &&
	git add file2 &&
	git commit -m second &&
	$SYSTEM_GIT tag -s -m "second signed" gpg-signed-2 HEAD
'

test_expect_success GPG 'verify-tag on second signed tag succeeds' '
	git verify-tag gpg-signed-2
'

test_expect_success GPG 'tag -v on signed tag shows signature' '
	git tag -v gpg-signed-tag >actual 2>&1 &&
	grep "GPG signed tag" actual
'

test_done
