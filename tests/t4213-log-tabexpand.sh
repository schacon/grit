#!/bin/sh
test_description='log/show --expand-tabs'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

HT="	"

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	test_tick &&
	sed -e "s/Q/$HT/g" <<-EOFMSG >msg &&
	Qtab indent at the beginning of the title line

	Qtab indent on a line in the body
	EOFMSG
	git commit --allow-empty -F msg
'

test_expect_success 'log shows commit with tab in message' '
	cd repo &&
	git log -n 1 >output &&
	grep "tab indent" output
'

test_done
