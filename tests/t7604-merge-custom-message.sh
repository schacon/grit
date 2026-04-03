#!/bin/sh

test_description='git merge with custom message'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init merge-msg &&
	cd merge-msg &&
	echo c0 >c0.c &&
	git add c0.c &&
	test_tick &&
	git commit -m c0 &&
	git tag c0 &&
	echo c1 >c1.c &&
	git add c1.c &&
	test_tick &&
	git commit -m c1 &&
	git tag c1 &&
	git reset --hard c0 &&
	echo c2 >c2.c &&
	git add c2.c &&
	test_tick &&
	git commit -m c2 &&
	git tag c2
'

test_expect_success 'merge c2 with a custom message' '
	cd merge-msg &&
	git reset --hard c1 &&
	git merge -m "custom message" c2 &&
	git cat-file commit HEAD >raw &&
	sed -e "1,/^$/d" raw >actual &&
	echo "custom message" >expected &&
	test_cmp expected actual
'

test_done
