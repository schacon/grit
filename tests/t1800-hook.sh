#!/bin/sh

test_description='git-hook command'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'git hook run: basic' '
	cd repo &&
	mkdir -p .git/hooks &&
	write_script .git/hooks/test-hook <<-EOF &&
	echo Test hook
	EOF

	cat >expect <<-\EOF &&
	Test hook
	EOF
	git hook run test-hook >actual &&
	test_cmp expect actual
'

test_expect_success 'git hook run -- pass arguments' '
	cd repo &&
	mkdir -p .git/hooks &&
	write_script .git/hooks/test-hook <<-\EOF &&
	echo $1
	echo $2
	EOF

	cat >expect <<-\EOF &&
	arg1
	arg2
	EOF
	git hook run test-hook -- arg1 arg2 >actual &&
	test_cmp expect actual
'

test_expect_success 'git hook run: non-zero exit code is returned' '
	cd repo &&
	mkdir -p .git/hooks &&
	write_script .git/hooks/test-hook <<-EOF &&
	exit 1
	EOF

	test_expect_code 1 git hook run test-hook
'

test_expect_success 'git hook run: pre-commit hook runs' '
	cd repo &&
	mkdir -p .git/hooks &&
	write_script .git/hooks/pre-commit <<-EOF &&
	echo "pre-commit ran"
	EOF

	git hook run pre-commit >actual &&
	echo "pre-commit ran" >expect &&
	test_cmp expect actual
'

test_expect_success 'git hook run: hook receives stdin' '
	cd repo &&
	mkdir -p .git/hooks &&
	write_script .git/hooks/test-hook <<-\EOF &&
	cat
	EOF

	echo "hello from stdin" | git hook run test-hook >actual &&
	echo "hello from stdin" >expect &&
	test_cmp expect actual
'

test_done
