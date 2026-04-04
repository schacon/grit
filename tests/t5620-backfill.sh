#!/bin/sh

test_description='git backfill on partial clones'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup repo for object creation' '
	git init src &&
	mkdir -p src/a/b &&
	for i in 1 2 3
	do
		echo "Version $i" > src/file.$i.txt &&
		echo "Version $i" > src/a/file.$i.txt &&
		echo "Version $i" > src/a/b/file.$i.txt &&
		git -C src add . &&
		git -C src commit -m "Iteration $i" || return 1
	done
'

test_expect_success 'backfill command exists' '
	git -C src backfill 2>&1 || true
'

test_expect_success 'backfill on partial clone' '
	git clone --filter=blob:none "file://$(pwd)/src" backfill1 &&
	git -C backfill1 backfill
'

test_done
