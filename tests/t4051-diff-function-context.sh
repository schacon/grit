#!/bin/sh

test_description='diff function context'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	cat >hello.c <<-\EOF &&
	#include <stdio.h>

	int first_func(void)
	{
		printf("first\n");
		return 0;
	}

	int second_func(void)
	{
		printf("second\n");
		return 0;
	}
	EOF
	git add hello.c &&
	git commit -m initial
'

test_expect_success 'diff shows context around change' '
	sed -i "s/first/changed/" hello.c &&
	git diff >actual &&
	grep "first_func\|changed" actual
'

test_expect_success 'diff shows hunk header with function name' '
	grep "@@" actual
'

test_done
