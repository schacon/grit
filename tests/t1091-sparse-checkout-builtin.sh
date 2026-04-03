#!/bin/sh

test_description='sparse checkout builtin tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	(
		cd repo &&
		echo "initial" >a &&
		mkdir folder1 folder2 deep &&
		mkdir deep/deeper1 deep/deeper2 &&
		mkdir deep/deeper1/deepest &&
		cp a folder1 &&
		cp a folder2 &&
		cp a deep &&
		cp a deep/deeper1 &&
		cp a deep/deeper2 &&
		cp a deep/deeper1/deepest &&
		git add . &&
		git commit -m "initial"
	)
'

test_expect_failure 'git sparse-checkout list (not sparse)' '
	test_must_fail git -C repo sparse-checkout list 2>err &&
	test_grep "this worktree is not sparse" err
'

test_expect_success 'git sparse-checkout init' '
	git -C repo sparse-checkout init &&
	git -C repo config core.sparseCheckout >actual &&
	grep "true" actual
'

test_expect_success 'git sparse-checkout list after init' '
	git -C repo sparse-checkout list >actual &&
	test_file_not_empty actual
'

test_expect_success 'set sparse-checkout using builtin' '
	git -C repo sparse-checkout set folder1 deep/deeper1 &&
	git -C repo sparse-checkout list >actual &&
	cat >expect <<-\EOF &&
	folder1
	deep/deeper1
	EOF
	test_cmp expect actual
'

test_expect_success 'sparse-checkout set writes sparse-checkout file' '
	git -C repo sparse-checkout set folder1 folder2 &&
	cat repo/.git/info/sparse-checkout >actual &&
	grep "folder1" actual &&
	grep "folder2" actual
'

test_expect_success 'sparse-checkout disable' '
	git -C repo sparse-checkout disable &&
	test_path_is_file repo/a &&
	test_path_is_file repo/folder1/a &&
	test_path_is_file repo/folder2/a
'

test_expect_success 'sparse-checkout init in empty repo' '
	git init empty &&
	git -C empty sparse-checkout init &&
	git -C empty config core.sparseCheckout >actual &&
	grep "true" actual
'

test_expect_success 'cone mode: init and set' '
	git -C repo sparse-checkout init &&
	git -C repo sparse-checkout set deep/deeper1 &&
	git -C repo sparse-checkout list >actual &&
	grep "deep/deeper1" actual
'

test_expect_success 'cone mode: set with nested folders' '
	git -C repo sparse-checkout set deep/deeper1 deep/deeper2 &&
	git -C repo sparse-checkout list >actual &&
	grep "deep/deeper1" actual &&
	grep "deep/deeper2" actual
'

test_done
