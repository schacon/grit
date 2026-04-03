#!/bin/sh
# Ported from upstream git t8003-blame-corner-cases.sh

test_description='git blame corner cases'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init blame-corner &&
	cd blame-corner &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&
	echo A A A A A >one &&
	echo B B B B B >two &&
	echo C C C C C >tres &&
	echo ABC >mouse &&
	test_write_lines 1 2 3 4 5 6 7 8 9 >nine_lines &&
	test_write_lines 1 2 3 4 5 6 7 8 9 a >ten_lines &&
	git add one two tres mouse nine_lines ten_lines &&
	test_tick &&
	GIT_AUTHOR_NAME=Initial git commit -m Initial &&

	cat one >uno &&
	mv two dos &&
	cat one >>tres &&
	echo DEF >>mouse &&
	git add uno dos tres mouse &&
	test_tick &&
	GIT_AUTHOR_NAME=Second git commit -a -m Second &&

	echo GHIJK >>mouse &&
	git add mouse &&
	test_tick &&
	GIT_AUTHOR_NAME=Third git commit -m Third &&

	cat mouse >cow &&
	git add cow &&
	test_tick &&
	GIT_AUTHOR_NAME=Fourth git commit -m Fourth &&

	cat >cow <<-\EOF &&
	ABC
	DEF
	XXXX
	GHIJK
	EOF
	git add cow &&
	test_tick &&
	GIT_AUTHOR_NAME=Fifth git commit -m Fifth
'

test_expect_success 'straight copy without -C' '
	cd blame-corner &&
	git blame uno | grep Second
'

test_expect_success 'blame moved file runs without error' '
	cd blame-corner &&
	git blame dos >actual &&
	test -s actual
'

test_expect_success 'blame -L with valid range' '
	cd blame-corner &&
	git blame -L 1,1 tres >out &&
	test_line_count = 1 out
'

test_expect_success 'blame -L with end beyond file' '
	cd blame-corner &&
	git blame -L 1,5 tres >out &&
	test_line_count = 2 out
'

test_expect_success 'indent of line numbers, nine lines' '
	cd blame-corner &&
	git blame nine_lines >actual &&
	test $(grep -c "  " actual) = 0
'

test_expect_success 'indent of line numbers, ten lines' '
	cd blame-corner &&
	git blame ten_lines >actual &&
	test $(grep -c "  " actual) = 9
'

test_expect_success 'blame --porcelain shows fields' '
	cd blame-corner &&
	git blame --porcelain uno >actual &&
	grep "^author " actual &&
	grep "^author-mail " actual &&
	grep "^author-time " actual &&
	grep "^committer " actual &&
	grep "^filename " actual &&
	grep "^summary " actual
'

test_expect_success 'blame --line-porcelain repeats headers' '
	cd blame-corner &&
	git blame --line-porcelain mouse >actual &&
	test $(grep -c "^author " actual) -eq 3
'

test_expect_success 'blame shows author on moved file' '
	cd blame-corner &&
	git blame dos >actual &&
	grep -E "(Second|Initial)" actual
'

test_expect_success 'blame shows correct line content' '
	cd blame-corner &&
	git blame uno >actual &&
	grep "A A A A A" actual
'

test_done
