#!/bin/sh
# Ported from upstream git t8005-blame-i18n.sh

test_description='git blame encoding'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init blame-i18n &&
	cd blame-i18n &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "UTF-8 LINE" >file &&
	git add file &&
	test_tick &&
	git commit -m "Initial UTF-8 commit" &&
	echo "Second LINE" >>file &&
	git add file &&
	test_tick &&
	git commit -m "Second commit"
'

test_expect_success 'blame shows author names' '
	cd blame-i18n &&
	git blame file >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'blame --porcelain shows encoding fields' '
	cd blame-i18n &&
	git blame --porcelain file >actual &&
	grep "^author " actual &&
	grep "^summary " actual
'

test_expect_success 'blame --line-porcelain works' '
	cd blame-i18n &&
	git blame --line-porcelain file >actual &&
	test $(grep -c "^author " actual) -eq 2
'

test_expect_success 'blame on single line' '
	cd blame-i18n &&
	git blame -L 1,1 file >actual &&
	test $(wc -l <actual) -eq 1
'

test_done
