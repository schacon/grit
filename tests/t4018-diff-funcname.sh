#!/bin/sh

test_description='Test custom diff function name patterns'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo A >A.java &&
	echo B >B.java &&
	git add A.java B.java &&
	git commit -m initial
'

test_expect_success 'diff shows function name context in hunk headers' '
	cat >file.c <<-\EOF &&
	int main() {
	    int x = 1;
	    int y = 2;
	    int z = 3;
	    return 0;
	}
	EOF
	git add file.c &&
	git commit -m "add C file" &&
	sed -e "s/int y = 2/int y = 42/" file.c >file.c.new &&
	mv file.c.new file.c &&
	git diff >output &&
	grep "^@@" output
'

test_done
