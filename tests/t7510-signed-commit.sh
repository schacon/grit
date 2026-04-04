#!/bin/sh
# Ported from upstream git t7510-signed-commit.sh
# Tests GPG-signed commits: structure tests and signature presence checks.

test_description='signed commit tests'

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

# /usr/bin/git is the system git — needed for -S (GPG signing) which grit
# does not implement natively.
SYSTEM_GIT=/usr/bin/git

test_expect_success 'setup' '
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'log --format=%H shows full hash' '
	git log --format=%H >actual &&
	test $(wc -c <actual) -gt 40
'

test_expect_success 'cat-file commit shows no gpgsig without signing' '
	git cat-file -p HEAD >actual &&
	! grep "^gpgsig" actual
'

test_expect_success 'multiple commits and verify log' '
	echo more >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m second &&
	echo even_more >file3 &&
	git add file3 &&
	test_tick &&
	git commit -m third &&
	git log --oneline >actual &&
	test_line_count = 3 actual
'

test_expect_success 'show commit message' '
	git log --max-count=1 --format=%s HEAD >actual &&
	grep "third" actual
'

test_expect_success 'rev-parse works on all commits' '
	git rev-parse HEAD >actual &&
	test -s actual &&
	git rev-parse HEAD^ >actual &&
	test -s actual &&
	git rev-parse HEAD~2 >actual &&
	test -s actual
'

# ── GPG signing tests ─────────────────────────────────────────────────────

test_expect_success GPG 'create signed commit with system git' '
	git config user.signingkey $GPG_KEY &&
	echo signed-content >signed-file &&
	git add signed-file &&
	test_tick &&
	$SYSTEM_GIT commit -S -m "signed commit"
'

test_expect_success GPG 'cat-file shows gpgsig header on signed commit' '
	git cat-file -p HEAD >actual &&
	grep "^gpgsig " actual
'

test_expect_success GPG 'verify-commit succeeds on signed commit' '
	git verify-commit HEAD
'

test_expect_success GPG 'verify-commit verbose shows commit contents' '
	git verify-commit -v HEAD >actual 2>&1 &&
	grep "signed commit" actual
'

test_expect_success GPG 'unsigned commit has no gpgsig header' '
	echo unsigned >unsigned-file &&
	git add unsigned-file &&
	test_tick &&
	git commit -m "unsigned commit" &&
	git cat-file -p HEAD >actual &&
	! grep "^gpgsig" actual
'

test_expect_success GPG 'signed commit contains PGP signature in object' '
	echo s1 >s1 &&
	git add s1 &&
	test_tick &&
	$SYSTEM_GIT commit -S -m "first signed" &&
	git tag first-signed &&
	git cat-file -p HEAD >actual &&
	grep "BEGIN PGP SIGNATURE" actual
'

test_expect_success GPG 'create multiple signed commits' '
	echo s2 >s2 &&
	git add s2 &&
	test_tick &&
	$SYSTEM_GIT commit -S -m "second signed" &&
	git tag second-signed
'

test_expect_success GPG 'gpgsig header distinguishes signed from unsigned' '
	git cat-file -p first-signed >actual &&
	grep "^gpgsig " actual &&
	git cat-file -p second-signed >actual &&
	grep "^gpgsig " actual
'

test_expect_success GPG 'amend and re-sign preserves gpgsig header' '
	git checkout -b amend-test first-signed &&
	$SYSTEM_GIT commit --amend -S --no-edit &&
	git cat-file -p HEAD >actual &&
	grep "^gpgsig " actual
'

test_expect_success GPG 'commit-tree with -S creates signed commit' '
	tree=$(git write-tree) &&
	oid=$($SYSTEM_GIT commit-tree -S -m "commit-tree signed" $tree) &&
	git cat-file -p $oid >actual &&
	grep "^gpgsig " actual
'

test_expect_success GPG 'verify-commit on signed commit via verify-commit' '
	git verify-commit first-signed &&
	git verify-commit second-signed
'

test_done
