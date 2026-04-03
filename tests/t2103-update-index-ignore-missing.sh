#!/bin/sh

test_description='update-index with options'

. ./test-lib.sh

test_expect_success 'basics: need --add when adding' '
	>one &&
	test_must_fail git update-index one &&
	test -z "$(git ls-files)" &&
	git update-index --add one &&
	test zone = "z$(git ls-files)"
'

test_expect_success 'update-index --add multiple files' '
	>two &&
	>three &&
	git update-index --add one two three &&
	test_write_lines one three two >expect &&
	git ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'update-index is atomic on error' '
	echo 1 >one &&
	test_must_fail git update-index one two_nonexistent &&
	echo "M	one" >expect &&
	git diff-files --name-status >actual &&
	test_cmp expect actual
'

test_expect_success 'update-index --ignore-missing' '
	rm -f two &&
	git update-index --ignore-missing one nonexistent 2>err &&
	test_must_be_empty err
'

test_done
