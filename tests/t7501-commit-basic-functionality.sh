#!/bin/sh
test_description='git commit'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_tick

test_expect_success 'setup: initial commit' '
	echo bongo bongo >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'nothing to commit' '
	git reset --hard &&
	test_must_fail git commit -m initial
'

test_expect_success 'setup: non-initial commit' '
	echo bongo bongo bongo >file &&
	git commit -m next -a
'

test_expect_success 'commit message from non-existing file' '
	echo more bongo: bongo bongo bongo bongo >file &&
	test_must_fail git commit -F gah -a
'

test_expect_success 'setup: commit message from file' '
	git checkout HEAD file && echo >>file && git add file &&
	echo this is the commit message, coming from a file >msg &&
	git commit -F msg -a
'

test_expect_success 'amend commit' '
	cat >editor <<-\EOF &&
	#!/bin/sh
	sed -e "s/a file/an amend commit/g" <"$1" >"$1-"
	mv "$1-" "$1"
	EOF
	chmod 755 editor &&
	EDITOR=./editor git commit --amend
'

test_expect_success 'message from stdin' '
	echo silly new contents >file &&
	echo commit message from stdin |
	git commit -F - -a
'

test_expect_success 'multiple -m' '
	>negative &&
	git add negative &&
	git commit -m "one" -m "two" -m "three" &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^\$/d" commit >actual &&
	(
		echo one &&
		echo &&
		echo two &&
		echo &&
		echo three
	) >expected &&
	test_cmp expected actual
'

test_expect_success 'same tree (single parent)' '
	git reset --hard &&
	test_must_fail git commit -m empty
'

test_expect_success 'same tree (single parent) --allow-empty' '
	git commit --allow-empty -m "forced empty" &&
	git cat-file commit HEAD >commit &&
	grep forced commit
'

test_done
