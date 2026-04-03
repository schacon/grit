#!/bin/sh

test_description='blame with revision tracking (basic tests)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-ignore &&
	cd blame-ignore &&
	echo line1 >file &&
	git add file &&
	test_tick &&
	git commit -m A &&
	git tag A &&

	echo line2 >>file &&
	git add file &&
	test_tick &&
	git commit -m B &&
	git tag B &&

	test_write_lines line-one line-two >file &&
	git add file &&
	test_tick &&
	git commit -m X &&
	git tag X
'

test_expect_success 'blame shows latest modifier' '
	cd blame-ignore &&
	git blame --line-porcelain file >blame_raw &&
	sed -ne "/^[0-9a-f]* [0-9]* 1/s/ .*//p" blame_raw >actual &&
	git rev-parse X >expect &&
	test_cmp expect actual
'

test_expect_success 'blame shows correct line count' '
	cd blame-ignore &&
	git blame file >output &&
	test $(wc -l <output) -eq 2
'

test_expect_success 'blame --porcelain shows author info' '
	cd blame-ignore &&
	git blame --porcelain file >output &&
	grep "^author " output
'

test_done
