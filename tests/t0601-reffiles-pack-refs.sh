#!/bin/sh
#
# Ported from git/t/t0601-reffiles-pack-refs.sh + pack-refs-tests.sh

test_description='git pack-refs should not change the branch semantic'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'prepare a trivial repository' '
	echo Hello > A &&
	git update-index --add A &&
	test_tick &&
	git commit -m "Initial commit." &&
	git rev-parse --verify HEAD >head_oid
'

test_expect_success 'pack-refs --all creates packed-refs' '
	test_path_is_missing .git/packed-refs &&
	git pack-refs --all &&
	test_path_is_file .git/packed-refs
'

test_expect_success 'see if git show-ref works as expected' '
	git branch a &&
	SHA1=$(cat .git/refs/heads/a) &&
	echo "$SHA1 refs/heads/a" >expect &&
	git show-ref a >result &&
	test_cmp expect result
'

test_expect_success 'see if a branch still exists when packed' '
	SHA1=$(cat head_oid) &&
	git branch b &&
	git pack-refs --all &&
	rm -f .git/refs/heads/b &&
	echo "$SHA1 refs/heads/b" >expect &&
	git show-ref b >result &&
	test_cmp expect result
'

test_done
