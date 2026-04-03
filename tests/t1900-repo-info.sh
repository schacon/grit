#!/bin/sh

test_description='test git repo info basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# The git repo info command is not implemented in grit.
# Test basic repository inspection via other commands.

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'rev-parse --git-dir shows .git' '
	cd repo &&
	git rev-parse --git-dir >actual &&
	grep ".git" actual
'

test_expect_success 'rev-parse --is-bare-repository for non-bare' '
	cd repo &&
	git rev-parse --is-bare-repository >actual &&
	echo "false" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --is-bare-repository for bare' '
	git init --bare bare-repo &&
	git -C bare-repo rev-parse --is-bare-repository >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_done
