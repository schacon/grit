#!/bin/sh

test_description='diff function name patterns'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup C file' '
	cat >hello.c <<-\EOF &&
	#include <stdio.h>

	int main(int argc, const char **argv)
	{
		printf("Hello world.\n");
		return 0;
	}
	EOF
	git add hello.c &&
	git commit -m initial
'

test_expect_success 'diff shows function name in hunk header' '
	sed -i "s/Hello world/Goodbye world/" hello.c &&
	git diff >actual &&
	grep "@@" actual &&
	grep "Hello\|Goodbye" actual
'

test_done
