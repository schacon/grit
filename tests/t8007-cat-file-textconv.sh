#!/bin/sh
# Ported from upstream git t8007-cat-file-textconv.sh

test_description='git cat-file textconv support'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init catconv &&
	cd catconv &&
	git config user.name "Test" &&
	git config user.email "test@example.com" &&
	echo "bin: test" >one.bin &&
	git add one.bin &&
	GIT_AUTHOR_NAME=Number1 git commit -a -m First --date="2010-01-01 18:00:00" &&
	echo "bin: test version 2" >one.bin &&
	GIT_AUTHOR_NAME=Number2 git commit -a -m Second --date="2010-01-01 20:00:00"
'

test_expect_success 'cat-file blob shows content' '
	cd catconv &&
	echo "bin: test version 2" >expected &&
	git cat-file blob HEAD:one.bin >result &&
	test_cmp expected result
'

test_expect_success 'cat-file -p shows content' '
	cd catconv &&
	git cat-file -p HEAD:one.bin >result &&
	echo "bin: test version 2" >expected &&
	test_cmp expected result
'

test_expect_success 'cat-file -t on blob' '
	cd catconv &&
	echo "blob" >expected &&
	git cat-file -t HEAD:one.bin >result &&
	test_cmp expected result
'

test_expect_success 'cat-file -s on blob' '
	cd catconv &&
	git cat-file -s HEAD:one.bin >result &&
	test -s result
'

test_expect_success 'cat-file usage: <bad rev>' '
	cd catconv &&
	test_must_fail git cat-file -t NOSUCHREV 2>actual
'

test_expect_success 'cat-file usage: <rev>:<bad path>' '
	cd catconv &&
	test_must_fail git cat-file -t HEAD:two.bin 2>actual
'

test_expect_success 'cat-file on commit' '
	cd catconv &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file -p on commit shows structure' '
	cd catconv &&
	git cat-file -p HEAD >actual &&
	grep "^tree " actual &&
	grep "^parent " actual &&
	grep "^author " actual
'

test_done
