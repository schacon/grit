#!/bin/sh
# Ported from git/t/t5351-unpack-large-objects.sh

test_description='git unpack-objects with large objects'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'create objects and pack' '
	git init -q &&
	echo "large content here" >big-blob &&
	git add big-blob &&
	git commit -m foo &&
	echo "more large content" >big-blob &&
	git add big-blob &&
	git commit -m bar &&
	PACK=$(echo HEAD | git pack-objects --revs pack) &&
	echo "$PACK" >pack-name &&
	git verify-pack -v pack-$PACK.pack >out
'

test_expect_success 'unpack-objects into bare repo' '
	PACK=$(cat pack-name) &&
	git init --bare dest.git &&
	git -C dest.git unpack-objects <pack-$PACK.pack
'

test_expect_success 'unpack-objects dry-run' '
	PACK=$(cat pack-name) &&
	git init --bare dest2.git &&
	git -C dest2.git unpack-objects -n <pack-$PACK.pack &&
	test $(find dest2.git/objects -type f | wc -l) -eq 0
'

test_done
