#!/bin/sh
#
# Copyright (c) 2007 Kristian Høgsberg <krh@redhat.com>
#

test_description='git commit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- helpers not in our test-lib.sh ---
test_grep () {
	if test "$1" = "-e"
	then
		shift
	fi
	grep "$@"
}

test_write_lines () {
	printf '%s\n' "$@"
}

author='The Real Author <someguy@his.email.org>'

test_tick

test_expect_success 'setup: init repo' '
	git init repo &&
	cd repo &&
	git config user.name "C O Mitter" &&
	git config user.email "committer@example.com"
'

test_expect_success 'setup: initial commit' '
	cd repo &&
	echo bongo bongo >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'fail initial amend on empty repo' '
	git init empty-repo &&
	cd empty-repo &&
	git config user.name "C O Mitter" &&
	git config user.email "committer@example.com" &&
	test_must_fail git commit --amend
'

# grit does not enforce -m/-F mutual exclusion
test_expect_failure '-m and -F do not mix' '
	cd repo &&
	git checkout HEAD file && echo >>file && git add file &&
	test_must_fail git commit -m foo -m bar -F file
'

test_expect_success 'nothing to commit' '
	cd repo &&
	git reset --hard &&
	test_must_fail git commit -m initial
'

test_expect_success '--dry-run fails with nothing to commit' '
	cd repo &&
	test_must_fail git commit -m initial --dry-run
'

test_expect_success 'setup: non-initial commit' '
	cd repo &&
	echo bongo bongo bongo >file &&
	git commit -m next -a
'

test_expect_success '--dry-run with stuff to commit returns ok' '
	cd repo &&
	echo bongo bongo bongo >>file &&
	git commit -m next -a --dry-run
'

test_expect_success 'commit message from non-existing file' '
	cd repo &&
	echo more bongo: bongo bongo bongo bongo >file &&
	test_must_fail git commit -F gah -a
'

test_expect_success 'setup: commit message from file' '
	cd repo &&
	git checkout HEAD file && echo >>file && git add file &&
	echo this is the commit message, coming from a file >msg &&
	git commit -F msg -a
'

test_expect_success 'amend commit' '
	cd repo &&
	echo amended >file &&
	git add file &&
	git commit --amend -m "amended commit"
'

test_expect_success 'multiple -m' '
	cd repo &&
	echo negative >negative &&
	git add negative &&
	git commit -m "one" -m "two" -m "three" &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^\$/d" commit >actual &&
	{
		echo one &&
		echo &&
		echo two &&
		echo &&
		echo three
	} >expected &&
	test_cmp expected actual
'

test_expect_success 'amend commit to fix author' '
	cd repo &&
	test_tick &&
	git reset --hard &&
	git commit --amend --author="$author" &&
	git cat-file -p HEAD >current &&
	grep "author The Real Author <someguy@his.email.org>" current
'

test_expect_success 'overriding author from command line' '
	cd repo &&
	echo gak >file &&
	git commit -m author \
		--author "Rubber Duck <rduck@convoy.org>" -a &&
	git cat-file -p HEAD >current &&
	grep "Rubber Duck" current
'

test_expect_success 'message from stdin' '
	cd repo &&
	echo silly new contents >file &&
	echo commit message from stdin |
	git commit -F - -a
'

test_expect_success 'same tree (single parent)' '
	cd repo &&
	git reset --hard &&
	test_must_fail git commit -m empty
'

test_expect_success 'same tree (single parent) --allow-empty' '
	cd repo &&
	git commit --allow-empty -m "forced empty" &&
	git cat-file commit HEAD >commit &&
	grep forced commit
'

test_expect_success 'commit --allow-empty-message' '
	cd repo &&
	echo something >file2 &&
	git add file2 &&
	git commit --allow-empty-message -m ""
'

test_expect_success 'commit --signoff' '
	cd repo &&
	echo signed >positive &&
	git add positive &&
	git commit -s -m "thank you" &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^\$/d" commit >actual &&
	grep "Signed-off-by:" actual
'

test_expect_success 'sign off (2) - existing signoff gets another one appended' '
	cd repo &&
	echo 2 >positive &&
	git add positive &&
	existing="Signed-off-by: Watch This <watchthis@example.com>" &&
	git commit -s -m "thank you

$existing" &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^\$/d" commit >actual &&
	grep "Watch This" actual &&
	test "$(grep -c "Signed-off-by:" actual)" = "2"
'

test_expect_success 'commit --reuse-message' '
	cd repo &&
	echo reuse >file2 &&
	git add file2 &&
	git commit --reuse-message=HEAD -q &&
	git cat-file commit HEAD >commit &&
	git cat-file commit HEAD^ >prev_commit &&
	sed -e "1,/^\$/d" commit >msg1 &&
	sed -e "1,/^\$/d" prev_commit >msg2 &&
	test_cmp msg2 msg1
'

test_expect_success 'commit --amend --allow-empty' '
	cd repo &&
	git commit --allow-empty -m "original" &&
	git commit --amend --allow-empty -m "amended" &&
	git log --format="%s" -n 1 >actual &&
	echo "amended" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit --date' '
	cd repo &&
	echo dated >dated &&
	git add dated &&
	git commit --date="2010-01-02T03:04:05" -m "dated commit" &&
	git cat-file -p HEAD >commit &&
	grep "author.*2010" commit || grep "1262401445" commit
'

test_expect_success 'commit -a' '
	cd repo &&
	echo modified >positive &&
	git commit -a -m "commit all"
'

test_expect_success 'commit with empty message fails without --allow-empty-message' '
	cd repo &&
	echo conga >file2 &&
	git add file2 &&
	test_must_fail git commit -m ""
'

test_expect_success 'partial commit via rm and add' '
	cd repo &&
	git rm --cached file2 2>/dev/null; true &&
	echo elif >elif &&
	git add elif &&
	git commit -a -m "add elif" &&
	git diff-tree --name-status HEAD^ HEAD >current &&
	grep "A" current
'

test_expect_success 'commit -F -' '
	cd repo &&
	echo more stuff >file2 &&
	git add file2 &&
	echo "message from pipe" | git commit -F - &&
	git log --format="%s" -n 1 >actual &&
	echo "message from pipe" >expected &&
	test_cmp expected actual
'

test_done
