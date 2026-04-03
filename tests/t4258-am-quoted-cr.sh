#!/bin/sh

test_description='test am with various patch formats'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	test_write_lines one two three >text &&
	git add text &&
	git commit -m one &&
	git tag one &&

	test_write_lines one owt three >text &&
	git commit -a -m two &&
	git tag two &&
	git format-patch -1 --stdout >two.patch
'

test_expect_success 'am applies patch changing middle line' '
	cd repo &&
	git reset --hard one &&
	git am two.patch &&
	test_write_lines one owt three >expect &&
	test_cmp expect text
'

test_expect_success 'am preserves commit info' '
	cd repo &&
	git log -n 1 --format=%s >actual &&
	echo "two" >expect &&
	test_cmp expect actual
'

test_expect_success 'am works after reset and reapply' '
	cd repo &&
	git reset --hard one &&
	git am two.patch &&
	git diff --exit-code two
'

test_done
