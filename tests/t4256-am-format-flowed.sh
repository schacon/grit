#!/bin/sh

test_description='test format-patch and am round-trip'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo "original content" >file &&
	git add file &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'format-patch | am round trip' '
	cd repo &&
	echo "modified content" >file &&
	git commit -a -m "modify file" &&
	git format-patch -1 --stdout >patch.mbox &&
	git reset --hard initial &&
	git am patch.mbox &&
	echo "modified content" >expect &&
	test_cmp expect file
'

test_expect_success 'format-patch preserves subject in am' '
	cd repo &&
	git log -n 1 --format=%s >actual &&
	echo "modify file" >expect &&
	test_cmp expect actual
'

test_done
