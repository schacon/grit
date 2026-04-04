#!/bin/sh
# Ported from git/t/t5544-pack-objects-hook.sh
# Tests custom script in place of pack-objects
# Grit does not support uploadpack.packObjectsHook

test_description='test custom script in place of pack-objects'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit one &&
	test_commit two
'

# grit does not support uploadpack.packObjectsHook
test_expect_success 'hook runs via global config' '
	write_script .git/hook <<-\EOF &&
		echo >&2 "hook running"
		echo "$*" >hook.args
		cat >hook.stdin
		"$@" <hook.stdin >hook.stdout
		cat hook.stdout
	EOF
	git -c uploadpack.packObjectsHook=.git/hook clone --no-local . dst.git 2>stderr &&
	grep "hook running" stderr
'

test_expect_success 'basic clone works without hook' '
	git clone --no-local . basic-clone &&
	git -C basic-clone rev-parse HEAD >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'clone preserves commits' '
	git -C basic-clone log --oneline >actual &&
	test_line_count = 2 actual
'

test_done
