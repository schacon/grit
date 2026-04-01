#!/bin/sh
# Ported subset from git/t/t2005-checkout-index-symlinks.sh

test_description='grit checkout-index core.symlinks false'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'prepare symlink entry with core.symlinks=false' '
	grit init repo &&
	cd repo &&
	cat >.git/config <<-\EOF &&
[core]
	repositoryformatversion = 0
	filemode = true
	bare = false
	symlinks = false
EOF
	l=$(printf file | grit hash-object -t blob -w --stdin) &&
	echo "$l" >symlink_oid &&
	printf "120000 %s\tsymlink\n" "$l" | grit update-index --index-info
'

test_expect_success 'checkout-index writes plain file instead of symlink' '
	cd repo &&
	grit checkout-index symlink &&
	test -f symlink &&
	! test -L symlink
'

test_expect_success 'checked out file matches stored blob' '
	cd repo &&
	l=$(cat symlink_oid) &&
	echo "$l" >expect &&
	grit hash-object -t blob symlink >actual &&
	test_cmp expect actual
'

test_done
