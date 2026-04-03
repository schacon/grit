#!/bin/sh

test_description='diff function context'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	cat >file.c <<-\EOF &&
	/* Hello comment */
	void hello() {
	    printf("hello\n");
	    /* delete me from hello */
	    printf("world\n");
	}

	void dummy() {
	    printf("dummy\n");
	}
	EOF
	git add file.c &&
	git commit -m initial &&
	git tag initial &&
	grep -v "delete me from hello" <file.c >file.c.new &&
	mv file.c.new file.c &&
	git add file.c &&
	git commit -m changed &&
	git tag changed
'

test_expect_success 'diff shows change in function' '
	git diff initial changed >output &&
	grep "delete me from hello" output
'

test_expect_success 'diff --stat shows changed file' '
	git diff --stat initial changed >output &&
	grep "file.c" output
'

test_expect_success 'diff-tree with patch shows deletion' '
	git diff-tree -p initial changed >output &&
	grep "^-.*delete me" output
'

test_done
