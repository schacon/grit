#!/bin/sh

test_description='git merge resolve strategy'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init merge-resolve &&
	cd merge-resolve &&
	echo c0 >c0.c &&
	git add c0.c &&
	test_tick &&
	git commit -m c0 &&
	git tag c0 &&
	echo c1 >c1.c &&
	git add c1.c &&
	test_tick &&
	git commit -m c1 &&
	git tag c1 &&
	git reset --hard c0 &&
	echo c2 >c2.c &&
	git add c2.c &&
	test_tick &&
	git commit -m c2 &&
	git tag c2 &&
	git reset --hard c0 &&
	echo c3 >c2.c &&
	git add c2.c &&
	test_tick &&
	git commit -m c3 &&
	git tag c3
'

test_expect_success 'merge c1 to c2' '
	cd merge-resolve &&
	git reset --hard c1 &&
	git merge c2 &&
	test "$(git rev-parse c1)" != "$(git rev-parse HEAD)" &&
	test "$(git rev-parse c1)" = "$(git rev-parse HEAD^1)" &&
	test "$(git rev-parse c2)" = "$(git rev-parse HEAD^2)" &&
	git diff --exit-code &&
	test_path_is_file c0.c &&
	test_path_is_file c1.c &&
	test_path_is_file c2.c &&
	test 3 = $(git ls-tree -r HEAD | wc -l) &&
	test 3 = $(git ls-files | wc -l)
'

test_expect_success 'merge c1 to c2 again' '
	cd merge-resolve &&
	git reset --hard c1 &&
	git merge c2 &&
	test "$(git rev-parse c1)" != "$(git rev-parse HEAD)" &&
	test "$(git rev-parse c1)" = "$(git rev-parse HEAD^1)" &&
	test "$(git rev-parse c2)" = "$(git rev-parse HEAD^2)"
'

test_done
