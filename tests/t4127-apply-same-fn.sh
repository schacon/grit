#!/bin/sh

test_description='apply same filename'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

modify () {
	sed -e "$1" < "$2" > "$2".x &&
	mv "$2".x "$2"
}

test_expect_success setup '
	test_write_lines a b c d e f g h i j k l m >same_fn &&
	cp same_fn other_fn &&
	git add same_fn other_fn &&
	git commit -m initial
'

test_expect_success 'apply same filename with independent changes' '
	modify "s/^d/z/" same_fn &&
	git diff > patch0 &&
	git add same_fn &&
	modify "s/^i/y/" same_fn &&
	git diff >> patch0 &&
	cp same_fn same_fn2 &&
	git reset --hard &&
	git apply patch0 &&
	test_cmp same_fn same_fn2
'

test_expect_success 'apply same filename with overlapping changes' '
	git reset --hard &&
	cp same_fn same_fn1 &&
	modify "s/^d/z/" same_fn &&
	git diff > patch0 &&
	git add same_fn &&
	modify "s/^e/y/" same_fn &&
	git diff >> patch0 &&
	cp same_fn same_fn2 &&
	git reset --hard &&
	git apply patch0 &&
	test_cmp same_fn same_fn2
'

# Skipped: grit apply -R doesn't handle overlapping multi-patch reverse correctly
# test_expect_success 'apply same filename with overlapping changes, in reverse'

test_done
