#!/bin/sh

test_description='Test diff indent heuristic.'

. ./test-lib.sh

test_expect_success 'prepare' '
	cat <<-\EOF >spaces.txt &&
	1
	2
	a

	b
	3
	4
	EOF

	cat <<-\EOF >functions.c &&
	1
	2
	/* function */
	foo() {
	    foo
	}

	3
	4
	EOF

	git add spaces.txt functions.c &&
	test_tick &&
	git commit -m initial &&
	git branch old &&

	cat <<-\EOF >spaces.txt &&
	1
	2
	a

	b
	a

	b
	3
	4
	EOF

	cat <<-\EOF >functions.c &&
	1
	2
	/* function */
	bar() {
	    foo
	}

	/* function */
	foo() {
	    foo
	}

	3
	4
	EOF

	git add spaces.txt functions.c &&
	test_tick &&
	git commit -m second &&
	git branch new
'

test_expect_success 'diff: basic output between old and new' '
	git diff old new -- spaces.txt >out &&
	grep "^+a" out &&
	grep "^+b" out
'

test_expect_success 'diff: functions output between old and new' '
	git diff old new -- functions.c >out &&
	grep "^+bar" out &&
	grep "foo()" out
'

# --indent-heuristic, --no-indent-heuristic are not implemented
test_expect_failure 'diff: --indent-heuristic (not implemented)' '
	git diff --indent-heuristic old new -- spaces.txt >out &&
	test -s out
'

test_expect_failure 'diff: --no-indent-heuristic (not implemented)' '
	git diff --no-indent-heuristic old new -- spaces.txt >out &&
	test -s out
'

test_done
