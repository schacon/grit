#!/bin/sh

test_description='git-pack-object with missing base'

. ./test-lib.sh

# Create A-B chain
test_expect_success 'setup base' '
	git init &&
	test_write_lines a b c d e f g h i >text &&
	echo side >side &&
	git update-index --add text side &&
	A=$(echo A | git commit-tree $(git write-tree)) &&
	echo m >>text &&
	git update-index text &&
	B=$(echo B | git commit-tree $(git write-tree) -p $A) &&
	git update-ref HEAD $B
'

test_expect_success 'pack-objects works with commit history' '
	echo HEAD | git pack-objects --revs testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows objects' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "commit" output &&
	test_grep "tree" output &&
	test_grep "blob" output
'

test_expect_success 'pack-objects --all includes everything' '
	git pack-objects --all allpack &&
	git verify-pack allpack-*.pack
'

test_done
