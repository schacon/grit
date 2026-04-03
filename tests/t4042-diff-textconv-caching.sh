#!/bin/sh

test_description='test textconv caching'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	echo foo content 1 >foo.bin &&
	echo bar content 1 >bar.bin &&
	git add . &&
	git commit -m one &&
	echo foo content 2 >foo.bin &&
	echo bar content 2 >bar.bin &&
	git commit -a -m two
'

test_expect_success 'diff between commits shows changes' '
	cd repo &&
	git diff HEAD~1 HEAD >actual &&
	grep "foo.bin" actual &&
	grep "bar.bin" actual
'

test_expect_success 'diff shows correct content changes' '
	cd repo &&
	git diff HEAD~1 HEAD >actual &&
	grep "+foo content 2" actual &&
	grep "+bar content 2" actual
'

test_done
