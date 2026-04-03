#!/bin/sh

test_description='more git add -u (typechange scenarios)'

. ./test-lib.sh

test_expect_success setup '
	>xyzzy &&
	_empty=$(git hash-object --stdin <xyzzy) &&
	>yomin &&
	>caskly &&
	>nitfol &&
	mkdir rezrov &&
	>rezrov/bozbar &&
	git add caskly xyzzy yomin nitfol rezrov/bozbar &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'add -u picks up modifications' '
	echo modified >nitfol &&
	git add -u &&
	git ls-files -s nitfol >actual &&
	test_line_count = 1 actual
'

test_expect_success 'add -u picks up removals' '
	rm -f caskly &&
	git add -u &&
	git ls-files caskly >actual &&
	test_must_be_empty actual
'

test_done
