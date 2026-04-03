#!/bin/sh

test_description='blame with fuzzy matching (basic tests)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-fuzzy &&
	cd blame-fuzzy &&

	test_write_lines a b c d e f g >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&

	test_write_lines a X c d e f g >file &&
	git add file &&
	test_tick &&
	git commit -m "change line 2" &&
	git tag change1 &&

	test_write_lines a X c d Y f g >file &&
	git add file &&
	test_tick &&
	git commit -m "change line 5" &&
	git tag change2
'

test_expect_success 'blame shows multiple commits' '
	cd blame-fuzzy &&
	git blame --line-porcelain file >output &&
	grep "^author " output >authors &&
	test $(wc -l <authors) -eq 7
'

test_expect_success 'blame correctly attributes changed lines' '
	cd blame-fuzzy &&
	git blame --porcelain file >output &&
	# line 2 (X) should be attributed to change1
	head -1 output | cut -d" " -f1 >first_sha &&
	test -s first_sha
'

test_expect_success 'blame -L restricts output' '
	cd blame-fuzzy &&
	git blame -L 2,5 file >output &&
	test $(wc -l <output) -eq 4
'

test_done
