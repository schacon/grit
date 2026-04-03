#!/bin/sh

test_description='apply empty'

. ./test-lib.sh

test_expect_success setup '
	>empty &&
	git add empty &&
	test_tick &&
	git commit -m initial &&
	test_write_lines a b c d e >empty &&
	cat empty >expect &&
	git diff |
	sed -e "/^diff --git/d" \
	    -e "/^index /d" \
	    -e "s|a/empty|empty.orig|" \
	    -e "s|b/empty|empty|" >patch0 &&
	>empty &&
	git update-index --refresh
'

test_expect_success 'apply empty' '
	test_when_finished "git reset --hard" &&
	git apply patch0 &&
	test_cmp expect empty
'

test_done
