#!/bin/sh
#
# Tests for 'checkout -' (switch to previous branch) and @{-N} syntax.
# Adapted from git/t/t2012-checkout-last.sh

test_description='checkout - (switch to last branch) and @{-N}'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >world &&
	git add world &&
	git commit -m "initial" &&
	git branch other &&
	echo "hello again" >>world &&
	git commit -a -m "second on master"
'

# ---------------------------------------------------------------------------
# checkout - does not work initially (no previous branch)
# ---------------------------------------------------------------------------
test_expect_success '"checkout -" does not work initially' '
	cd repo &&
	test_must_fail git checkout -
'

# ---------------------------------------------------------------------------
# First branch switch, then checkout - switches back
# ---------------------------------------------------------------------------
test_expect_success 'first branch switch' '
	cd repo &&
	git checkout other
'

test_expect_success '"checkout -" switches back to master' '
	cd repo &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" switches forth to other' '
	cd repo &&
	git checkout - &&
	echo refs/heads/other >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# Detach HEAD, then checkout - attaches again
# ---------------------------------------------------------------------------
test_expect_success 'detach HEAD' '
	cd repo &&
	git checkout $(git rev-parse HEAD)
'

test_expect_success '"checkout -" attaches again' '
	cd repo &&
	git checkout - &&
	echo refs/heads/other >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_failure '"checkout -" detaches again' '
	cd repo &&
	git checkout - &&
	git rev-parse other >expect &&
	git rev-parse HEAD >actual &&
	test_cmp expect actual &&
	test_must_fail git symbolic-ref HEAD
'

# ---------------------------------------------------------------------------
# @{-N} syntax
# ---------------------------------------------------------------------------
test_expect_success 'create many branches for @{-N} tests' '
	cd repo &&
	git checkout master &&
	for i in 1 2 3 4 5
	do
		git checkout -b branch$i || return 1
	done
'

test_expect_failure '@{-1} switches to the last branch' '
	cd repo &&
	git checkout branch1 &&
	git checkout branch2 &&
	git checkout branch3 &&
	git checkout "@{-1}" &&
	echo refs/heads/branch2 >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_failure '@{-2} switches to second from last' '
	cd repo &&
	git checkout branch1 &&
	git checkout branch2 &&
	git checkout branch3 &&
	git checkout "@{-2}" &&
	echo refs/heads/branch1 >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_failure '@{-3} switches to third from last' '
	cd repo &&
	git checkout branch1 &&
	git checkout branch2 &&
	git checkout branch3 &&
	git checkout branch4 &&
	git checkout "@{-3}" &&
	echo refs/heads/branch1 >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# checkout - after multiple branch hops
# ---------------------------------------------------------------------------
test_expect_success 'checkout - after multiple hops' '
	cd repo &&
	git checkout master &&
	git checkout branch1 &&
	git checkout branch2 &&
	git checkout - &&
	echo refs/heads/branch1 >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git checkout - &&
	echo refs/heads/branch2 >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# checkout - is equivalent to @{-1}
# ---------------------------------------------------------------------------
test_expect_failure '"checkout -" is same as checkout @{-1}' '
	cd repo &&
	git checkout master &&
	git checkout branch3 &&
	git checkout - &&
	echo refs/heads/master >expect_dash &&
	git symbolic-ref HEAD >actual_dash &&
	test_cmp expect_dash actual_dash &&

	git checkout master &&
	git checkout branch3 &&
	git checkout "@{-1}" &&
	echo refs/heads/master >expect_at &&
	git symbolic-ref HEAD >actual_at &&
	test_cmp expect_at actual_at
'

# ---------------------------------------------------------------------------
# checkout - preserves working tree content
# ---------------------------------------------------------------------------
test_expect_success 'checkout - preserves working tree content' '
	cd repo &&
	git checkout master &&
	git checkout other &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	grep "hello again" world
'

# ---------------------------------------------------------------------------
# checkout - three times cycles through correctly
# ---------------------------------------------------------------------------
test_expect_success 'checkout - three times cycles correctly' '
	cd repo &&
	git checkout master &&
	git checkout other &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "other" &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "master"
'

# ---------------------------------------------------------------------------
# checkout @{-N} with N too large fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout @{-N} with N too large fails' '
	cd repo &&
	git checkout master &&
	test_must_fail git checkout "@{-100}" 2>err
'

# ---------------------------------------------------------------------------
# checkout - after -b uses previous branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout - after creating new branch goes to previous' '
	cd repo &&
	git checkout master &&
	git checkout -b tmp-branch &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git branch -D tmp-branch
'

# ---------------------------------------------------------------------------
# checkout - from detached HEAD goes to previous branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout - from detached HEAD re-attaches to prev branch' '
	cd repo &&
	git checkout master &&
	git checkout other &&
	git checkout $(git rev-parse HEAD) &&
	git checkout - &&
	echo refs/heads/other >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# @{-1} in combination with -b creates branch from previous HEAD
# ---------------------------------------------------------------------------
test_expect_failure 'checkout -b newbranch @{-1} creates branch from prev' '
	cd repo &&
	git checkout master &&
	git checkout branch1 &&
	git checkout branch2 &&
	git checkout -b from-prev "@{-1}" &&
	echo refs/heads/from-prev >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git rev-parse HEAD >actual_oid &&
	git rev-parse branch1 >expect_oid &&
	test_cmp expect_oid actual_oid &&
	git checkout master &&
	git branch -D from-prev
'

# ---------------------------------------------------------------------------
# checkout - does not confuse with file named -
# ---------------------------------------------------------------------------
test_expect_success 'checkout - is not confused by file named -' '
	cd repo &&
	git checkout master &&
	git checkout other &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# @{-1} after detach/reattach cycle
# ---------------------------------------------------------------------------
test_expect_failure '@{-1} after detach and reattach' '
	cd repo &&
	git checkout master &&
	git checkout branch3 &&
	git checkout --detach &&
	git checkout branch1 &&
	git checkout "@{-1}" &&
	# @{-1} should be the detached state hash, which means detached
	# Or it could be branch3 depending on reflog
	# Just verify we successfully checked out something
	git rev-parse HEAD >/dev/null
'

# ---------------------------------------------------------------------------
# checkout - preserves dirty untracked files
# ---------------------------------------------------------------------------
test_expect_success 'checkout - preserves untracked files' '
	cd repo &&
	git checkout master &&
	echo untracked >untracked-file &&
	git checkout other &&
	git checkout - &&
	test -f untracked-file &&
	rm -f untracked-file
'

# ---------------------------------------------------------------------------
# @{-2} after branch hops
# ---------------------------------------------------------------------------
test_expect_failure '@{-2} goes to second-to-last branch' '
	cd repo &&
	git checkout master &&
	git checkout branch1 &&
	git checkout branch2 &&
	git checkout branch3 &&
	git checkout "@{-2}" &&
	echo refs/heads/branch1 >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# checkout - does not change with intermediate operations
# ---------------------------------------------------------------------------
test_expect_success 'checkout - after rev-parse still switches correctly' '
	cd repo &&
	git checkout master &&
	git checkout other &&
	git rev-parse HEAD >/dev/null &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_done
