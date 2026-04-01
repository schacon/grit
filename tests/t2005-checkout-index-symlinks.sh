#!/bin/sh
# Ported subset from git/t/t2005-checkout-index-symlinks.sh

test_description='gust checkout-index core.symlinks false'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'prepare symlink entry with core.symlinks=false' '
	gust init repo &&
	cd repo &&
	cat >.git/config <<-\EOF &&
[core]
	repositoryformatversion = 0
	filemode = true
	bare = false
	symlinks = false
EOF
	l=$(printf file | gust hash-object -t blob -w --stdin) &&
	echo "$l" >symlink_oid &&
	printf "120000 %s\tsymlink\n" "$l" | gust update-index --index-info
'

test_expect_success 'checkout-index writes plain file instead of symlink' '
	cd repo &&
	gust checkout-index symlink &&
	test -f symlink &&
	! test -L symlink
'

test_expect_success 'checked out file matches stored blob' '
	cd repo &&
	l=$(cat symlink_oid) &&
	echo "$l" >expect &&
	gust hash-object -t blob symlink >actual &&
	test_cmp expect actual
'

test_done
