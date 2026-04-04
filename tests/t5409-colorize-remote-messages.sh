#!/bin/sh

test_description='remote messages are colorized on the client'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_hook --setup update <<-\EOF &&
	echo error: error
	echo hint: hint
	echo success: success
	echo warning: warning
	exit 0
	EOF
	echo 1 >file &&
	git add file &&
	git commit -m 1 &&
	git clone . child &&
	(
		cd child &&
		echo 2 >file &&
		git add file &&
		git commit -m 2
	)
'

test_expect_success 'keywords' '
	git --git-dir child/.git -c color.remote=always push -f origin HEAD:refs/heads/keywords 2>output &&
	test_decode_color <output >decoded &&
	grep "<BOLD><RED>error<RESET>: error" decoded
'

test_done
