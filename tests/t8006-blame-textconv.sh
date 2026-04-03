#!/bin/sh

test_description='git blame textconv support (basic)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

find_blame() {
	sed -e 's/^[^(]*//'
}

test_expect_success 'setup' '
	git init blame-textconv &&
	cd blame-textconv &&
	echo "bin: test number 0" >zero.bin &&
	echo "bin: test 1" >one.bin &&
	echo "bin: test number 2" >two.bin &&
	git add . &&
	GIT_AUTHOR_NAME=Number1 git commit -a -m First &&
	echo "bin: test 1 version 2" >one.bin &&
	echo "bin: test number 2 version 2" >>two.bin &&
	GIT_AUTHOR_NAME=Number2 git commit -a -m Second
'

test_expect_success 'blame without textconv works' '
	cd blame-textconv &&
	git blame one.bin >blame &&
	grep "Number2" blame
'

test_expect_success 'blame shows correct content for multi-line file' '
	cd blame-textconv &&
	git blame two.bin >blame &&
	grep "Number1" blame &&
	grep "Number2" blame
'

test_expect_success 'blame --porcelain on binary-like files' '
	cd blame-textconv &&
	git blame --porcelain one.bin >output &&
	grep "^author Number2" output
'

test_done
