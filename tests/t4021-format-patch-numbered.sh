#!/bin/sh
#
# Copyright (c) 2006 Brian C Gernhardt
#

test_description='Format-patch numbering options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	echo A > file &&
	git add file &&
	git commit -m First &&

	echo B >> file &&
	git commit -a -m Second &&

	echo C >> file &&
	git commit -a -m Third
'

# Each of these gets used multiple times.

test_num_no_numbered() {
	cnt=$(grep "^Subject: \[PATCH\]" $1 | wc -l) &&
	test $cnt = $2
}

test_single_no_numbered() {
	test_num_no_numbered $1 1
}

test_no_numbered() {
	test_num_no_numbered $1 2
}

test_single_numbered() {
	grep "^Subject: \[PATCH 1/1\]" $1
}

test_numbered() {
	grep "^Subject: \[PATCH 1/2\]" $1 &&
	grep "^Subject: \[PATCH 2/2\]" $1
}

test_expect_success 'single patch defaults to no numbers' '
	cd repo &&
	git format-patch --stdout HEAD~1 >patch0.single &&
	test_single_no_numbered patch0.single
'

test_expect_success 'multiple patch defaults to numbered' '
	cd repo &&
	git format-patch --stdout HEAD~2 >patch0.multiple &&
	test_numbered patch0.multiple
'

test_expect_success 'Use --numbered' '
	cd repo &&
	git format-patch --numbered --stdout HEAD~1 >patch1 &&
	test_single_numbered patch1
'

test_expect_success 'format.numbered = true' '
	cd repo &&
	git config format.numbered true &&
	git format-patch --stdout HEAD~2 >patch2 &&
	test_numbered patch2
'

test_expect_success 'format.numbered = auto' '
	cd repo &&
	git config format.numbered auto &&
	git format-patch --stdout HEAD~2 > patch5 &&
	test_numbered patch5
'

test_expect_success 'format.numbered = auto && single patch' '
	cd repo &&
	git format-patch --stdout HEAD^ > patch6 &&
	test_single_no_numbered patch6
'

test_done
