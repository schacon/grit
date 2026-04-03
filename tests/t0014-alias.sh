#!/bin/sh

test_description='git command aliasing (basic tests only)'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

# Note: grit does not yet support aliases.
# These tests verify basic config alias storage and retrieval.

test_expect_success 'can store alias in config' '
	git config alias.st status &&
	echo status >expect &&
	git config alias.st >actual &&
	test_cmp expect actual
'

test_expect_success 'can store and retrieve alias with spaces' '
	git config alias.lg "log --oneline" &&
	echo "log --oneline" >expect &&
	git config alias.lg >actual &&
	test_cmp expect actual
'

test_expect_success 'alias without value reports error' '
	test_when_finished "git config --unset alias.noval 2>/dev/null; true" &&
	printf "[alias]\n\tnoval\n" >>.git/config &&
	git config alias.noval >output 2>&1 || true
'

test_done
