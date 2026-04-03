#!/bin/sh

test_description='format-patch -s should force MIME encoding as needed'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	>F &&
	git add F &&
	git commit -m initial &&
	echo new line >F &&
	git add F &&
	test_tick &&
	git commit -m "This adds some lines to F"
'

test_expect_success 'format normally' '
	cd repo &&
	git format-patch --stdout HEAD~1 >output &&
	! grep Content-Type output
'

test_done
