#!/bin/sh
# Ported from upstream git t8010-cat-file-filters.sh

test_description='git cat-file filters support'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init catfilter &&
	cd catfilter &&
	git config user.name "Test" &&
	git config user.email "test@example.com" &&
	echo "hello" >world.txt &&
	git add world.txt &&
	test_tick &&
	git commit -m "Initial commit"
'

test_expect_success 'cat-file blob works' '
	cd catfilter &&
	git cat-file blob HEAD:world.txt >actual &&
	echo "hello" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file -p works on blob' '
	cd catfilter &&
	git cat-file -p HEAD:world.txt >actual &&
	echo "hello" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file -t shows type' '
	cd catfilter &&
	git cat-file -t HEAD:world.txt >actual &&
	echo "blob" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file -s shows size' '
	cd catfilter &&
	git cat-file -s HEAD:world.txt >actual &&
	echo "6" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file on commit' '
	cd catfilter &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file -p on commit shows tree and author' '
	cd catfilter &&
	git cat-file -p HEAD >actual &&
	grep "^tree " actual &&
	grep "^author " actual &&
	grep "^committer " actual
'

test_done
