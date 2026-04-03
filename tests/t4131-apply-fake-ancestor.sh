#!/bin/sh
#
# Ported subset from git/t/t4131-apply-fake-ancestor.sh

test_description='git apply with multiple commits'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo 1 >file &&
	git add file &&
	git commit -m "commit 1" &&
	git tag tag1 &&
	echo 2 >file &&
	git commit -a -m "commit 2" &&
	git tag tag2 &&
	echo 3 >file &&
	git commit -a -m "commit 3" &&
	git tag tag3 &&
	echo 4 >file &&
	git commit -a -m "commit 4" &&
	git tag tag4
'

test_expect_success 'apply patch from earlier commit' '
	cd repo &&
	git checkout tag1 &&
	git diff tag1 tag2 >patch &&
	git apply patch &&
	echo 2 >expect &&
	test_cmp expect file
'

test_expect_success 'apply sequence of patches' '
	cd repo &&
	git reset --hard tag1 &&
	git diff tag1 tag2 >patch1 &&
	git diff tag2 tag3 >patch2 &&
	git apply patch1 &&
	git apply patch2 &&
	echo 3 >expect &&
	test_cmp expect file
'

test_done
