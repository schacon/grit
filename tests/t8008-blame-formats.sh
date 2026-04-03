#!/bin/sh
# Ported from upstream git t8008-blame-formats.sh
# blame output in various formats on a simple case

test_description='blame output in various formats on a simple case'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-fmt &&
	cd blame-fmt &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&
	echo a >file &&
	git add file &&
	test_tick &&
	git commit -m one &&
	echo b >>file &&
	echo c >>file &&
	echo d >>file &&
	test_tick &&
	git commit -a -m two
'

test_expect_success 'normal blame output' '
	cd blame-fmt &&
	git blame file >actual &&
	test $(wc -l <actual) -eq 4
'

test_expect_success 'blame --porcelain output' '
	cd blame-fmt &&
	git blame --porcelain file >actual &&
	grep "^author A U Thor" actual &&
	grep "^summary one" actual &&
	grep "^summary two" actual &&
	grep "^filename file" actual
'

test_expect_success 'blame --line-porcelain output' '
	cd blame-fmt &&
	git blame --line-porcelain file >actual &&
	# line-porcelain repeats headers for every line
	test $(grep -c "^author " actual) -eq 4
'

test_expect_success 'blame --porcelain has correct fields' '
	cd blame-fmt &&
	git blame --porcelain file >actual &&
	grep "^author-mail <author@example.com>" actual &&
	grep "^author-time " actual &&
	grep "^author-tz " actual &&
	grep "^committer " actual &&
	grep "^committer-mail " actual &&
	grep "^committer-time " actual &&
	grep "^committer-tz " actual
'

test_expect_success '--porcelain detects first non-blank line as subject' '
	cd blame-fmt &&
	TREE=$(git write-tree) &&
	commit=$(printf "%s\n%s\n%s\n\n\n  \noneline\n\nbody\n" \
		"tree $TREE" \
		"author A <a@b.c> 123456789 +0000" \
		"committer C <c@d.e> 123456789 +0000" |
	git hash-object -w -t commit --stdin) &&
	git blame --porcelain $commit -- file >output &&
	grep "^summary " output
'

test_done
