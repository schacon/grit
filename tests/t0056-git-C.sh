#!/bin/sh

test_description='"-C <path>" option and its effects on other path-related options'

. ./test-lib.sh

test_expect_success '"git -C <path>" runs git from the directory <path>' '
	test_create_repo dir1 &&
	echo 1 >dir1/a.txt &&
	msg="initial in dir1" &&
	(cd dir1 && git add a.txt && git commit -m "$msg") &&
	echo "$msg" >expected &&
	git -C dir1 log --format=%s >actual &&
	test_cmp expected actual
'

test_expect_success 'Relative followed by fullpath: "-C /there" works' '
	test_create_repo dir1/dir2 &&
	echo 1 >dir1/dir2/b.txt &&
	git -C dir1/dir2 add b.txt &&
	msg="initial in dir1/dir2" &&
	echo "$msg" >expected &&
	git -C dir1/dir2 commit -m "$msg" &&
	git -C "$(pwd)/dir1/dir2" log --format=%s >actual &&
	test_cmp expected actual
'

test_done
