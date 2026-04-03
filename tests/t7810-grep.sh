#!/bin/sh

test_description='git grep various'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

cat >hello.c <<EOF
#include <stdio.h>
int main(int argc, const char **argv)
{
	printf("Hello world.\n");
	return 0;
	/* char strstrstr = "strstrstrstrstr"; */
}
EOF

test_expect_success 'setup' '
	echo "foo mmap bar" >file &&
	echo "foo_mmap bar mmap" >file2 &&
	echo "foo mmap bar\n" >file3 &&
	cp hello.c hello_world.c &&
	git add file file2 file3 hello_world.c hello.c &&
	git commit -m initial
'

test_expect_success 'grep should not segfault with a bad input' '
	test_must_fail git grep "("
'

test_expect_success 'grep -l' '
	git grep -l mmap >actual &&
	grep file actual &&
	grep file2 actual
'

test_expect_success 'grep -i' '
	echo "HELLO WORLD" >mixed &&
	git add mixed &&
	git grep -i "hello world" >actual &&
	grep mixed actual
'

test_expect_success 'grep -n' '
	git grep -n "Hello" >actual &&
	grep "hello.c:4:" actual &&
	grep "hello_world.c:4:" actual
'

test_expect_success 'grep with fixed string' '
	git grep -F "foo mmap bar" >actual &&
	grep "file:" actual
'

test_expect_success 'grep in specific file' '
	git grep mmap -- file2 >actual &&
	grep file2 actual
'

test_expect_success 'grep --count' '
	git grep --count mmap >actual &&
	grep "file:" actual
'

test_done
