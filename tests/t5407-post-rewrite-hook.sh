#!/bin/sh

test_description='Test the post-rewrite hook.'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit A foo A &&
	test_commit B foo B &&
	test_commit C foo C &&
	test_commit D foo D &&

	test_hook --setup post-rewrite <<-EOF
	echo \$@ > "$TRASH_DIRECTORY"/post-rewrite.args
	cat > "$TRASH_DIRECTORY"/post-rewrite.data
	EOF
'

clear_hook_input () {
	rm -f post-rewrite.args post-rewrite.data
}

verify_hook_input () {
	test_cmp expected.args "$TRASH_DIRECTORY"/post-rewrite.args &&
	test_cmp expected.data "$TRASH_DIRECTORY"/post-rewrite.data
}

test_expect_failure 'git commit --amend fires post-rewrite hook' '
	clear_hook_input &&
	echo "D new message" > newmsg &&
	oldsha=$(git rev-parse HEAD^0) &&
	git commit -Fnewmsg --amend &&
	echo amend > expected.args &&
	echo $oldsha $(git rev-parse HEAD^0) > expected.data &&
	verify_hook_input
'

test_expect_failure 'git commit --amend --no-post-rewrite' '
	clear_hook_input &&
	echo "D new message again" > newmsg &&
	git commit --no-post-rewrite -Fnewmsg --amend &&
	test ! -f post-rewrite.args &&
	test ! -f post-rewrite.data
'

test_done
