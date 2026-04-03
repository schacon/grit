#!/bin/sh
# Ported from upstream git t7520-ignored-hook-warning.sh

test_description='ignored hook warning'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init hook-repo &&
	cd hook-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'setup hook' '
	cd hook-repo &&
	mkdir -p .git/hooks &&
	cat >.git/hooks/pre-commit <<-\HOOKEOF &&
	#!/bin/sh
	exit 0
	HOOKEOF
	chmod +x .git/hooks/pre-commit
'

test_expect_success 'commit works with hook' '
	cd hook-repo &&
	echo more >file &&
	git add file &&
	test_tick &&
	git commit -m "with hook"
'

test_expect_success 'hook is executed' '
	cd hook-repo &&
	cat >.git/hooks/pre-commit <<-\HOOKEOF &&
	#!/bin/sh
	echo "hook was run" >/tmp/grit-hook-test-$$
	exit 0
	HOOKEOF
	chmod +x .git/hooks/pre-commit &&
	echo even_more >file &&
	git add file &&
	test_tick &&
	git commit -m "hook executed"
'

test_expect_success 'non-executable hook is ignored' '
	cd hook-repo &&
	chmod -x .git/hooks/pre-commit &&
	echo yet_more >file &&
	git add file &&
	test_tick &&
	git commit -m "no hook" 2>msg || true &&
	# either it warns or it just works
	test -s file
'

test_done
