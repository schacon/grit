#!/bin/sh

test_description='apply a patch that is larger than the preimage'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup file with many lines' '
	cat >F <<-\EOF &&
	1
	2
	3
	4
	5
	6
	7
	8
	999999
	A
	B
	C
	D
	E
	F
	G
	H
	I
	J

	EOF
	git add F &&
	git commit -m "add F"
'

test_expect_success 'generate patch and modify file' '
	mv F G &&
	sed -e "s/1/11/" -e "s/999999/9/" -e "s/H/HH/" <G >F &&
	git diff >patch &&
	test -s patch
'

test_expect_success 'patch stat shows changes' '
	git apply --stat patch >output &&
	grep "F" output
'

test_done
