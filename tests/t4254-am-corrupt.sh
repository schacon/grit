#!/bin/sh

test_description='git am with various patch inputs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	echo a >f &&
	git add f &&
	test_tick &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'am applies well-formed patch' '
	cd repo &&
	echo b >f &&
	test_tick &&
	git commit -a -m "change to b" &&
	git format-patch -1 --stdout >good.patch &&
	git reset --hard initial &&
	git am good.patch &&
	echo b >expect &&
	test_cmp expect f
'

test_expect_success 'am preserves author information' '
	cd repo &&
	git log -n 1 --format="%an <%ae>" >actual &&
	echo "A U Thor <author@example.com>" >expect &&
	test_cmp expect actual
'

test_expect_success 'am preserves commit message' '
	cd repo &&
	git log -n 1 --format=%s >actual &&
	echo "change to b" >expect &&
	test_cmp expect actual
'

test_done
