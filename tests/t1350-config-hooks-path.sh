#!/bin/sh

test_description='Test the core.hooksPath configuration variable'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_failure 'Check that various forms of specifying core.hooksPath work' '
	>actual &&
	mkdir -p .git/custom-hooks &&
	write_script .git/custom-hooks/pre-commit <<-\EOF &&
	echo CUSTOM >>actual
	EOF
	test_hook --setup pre-commit <<-\EOF &&
	echo NORMAL >>actual
	EOF
	test_commit no_custom_hook &&
	git config core.hooksPath .git/custom-hooks &&
	test_commit have_custom_hook &&
	git config core.hooksPath .git/custom-hooks/ &&
	test_commit have_custom_hook_trailing_slash &&
	git config core.hooksPath "$PWD/.git/custom-hooks" &&
	test_commit have_custom_hook_abs_path &&
	git config core.hooksPath "$PWD/.git/custom-hooks/" &&
	test_commit have_custom_hook_abs_path_trailing_slash &&
	cat >expect <<-\EOF &&
	NORMAL
	CUSTOM
	CUSTOM
	CUSTOM
	CUSTOM
	EOF
	test_cmp expect actual
'

test_expect_failure 'git rev-parse --git-path hooks' '
	git config core.hooksPath .git/custom-hooks &&
	git rev-parse --git-path hooks/abc >actual &&
	test .git/custom-hooks/abc = "$(cat actual)"
'

test_expect_success 'core.hooksPath can be set and read' '
	git config core.hooksPath /custom/hooks &&
	val=$(git config core.hooksPath) &&
	test "$val" = "/custom/hooks"
'

test_expect_success 'core.hooksPath can be relative' '
	git config core.hooksPath .git/custom-hooks &&
	val=$(git config core.hooksPath) &&
	test "$val" = ".git/custom-hooks"
'

test_done
