#!/bin/sh
#
# Ported subset from git/t/t4115-apply-symlink.sh

test_description='git apply basic operations with file creation and deletion'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	echo "original" >file &&
	git add file &&
	git commit -m initial &&
	git tag initial &&

	echo "modified" >file &&
	git commit -a -m second &&
	git tag second &&

	git diff-tree -p initial second >patch &&
	git apply --stat --summary patch >stat_output
'

test_expect_success 'apply --stat shows diff stats' '
	cd repo &&
	test_grep "file" stat_output
'

test_expect_success 'apply patch on matching base' '
	cd repo &&
	git checkout initial &&
	git apply patch &&
	echo "modified" >expect &&
	test_cmp expect file
'

test_expect_success 'apply --check succeeds on matching base' '
	cd repo &&
	git reset --hard initial &&
	git apply --check patch
'

test_done
