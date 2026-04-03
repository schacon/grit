#!/bin/sh

test_description='read-tree with sparse checkout'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "init" >init.t &&
	mkdir sub subsub &&
	echo "sub" >sub/added &&
	echo "sub2" >sub/addedtoo &&
	echo "subsub" >subsub/added &&
	git add init.t sub/ subsub/ &&
	git commit -m "initial"
'

test_expect_success 'read-tree without sparse checkout works normally' '
	git read-tree -m -u HEAD &&
	git ls-files --stage >result &&
	grep "init.t" result &&
	grep "sub/added" result
'

test_expect_success 'ls-files -t shows status prefix for tracked files' '
	git ls-files -t >result &&
	grep "^H init.t" result &&
	grep "^H sub/added" result
'

test_expect_failure 'read-tree with empty sparse-checkout hides all files' '
	git config core.sparsecheckout true &&
	mkdir -p .git/info &&
	echo >.git/info/sparse-checkout &&
	git read-tree -m -u HEAD &&
	git ls-files -t >result &&
	grep "^S init.t" result &&
	test_path_is_missing init.t
'

test_expect_failure 'read-tree with sparse-checkout pattern selects files' '
	echo "sub/" >.git/info/sparse-checkout &&
	git read-tree -m -u HEAD &&
	git ls-files -t >result &&
	grep "^S init.t" result &&
	grep "^H sub/added" result &&
	test_path_is_missing init.t &&
	test_path_is_file sub/added
'

test_expect_success 'read-tree can reset to HEAD' '
	git config core.sparsecheckout false &&
	git read-tree --reset -u HEAD &&
	git ls-files --stage >result &&
	grep "init.t" result
'

test_expect_success 'read-tree can resolve tags' '
	git tag testtag &&
	git read-tree -m -u testtag &&
	git ls-files --stage >result &&
	grep "init.t" result
'

test_expect_success 'read-tree can resolve commit SHA' '
	SHA=$(git rev-parse HEAD) &&
	git read-tree -m -u $SHA &&
	git ls-files --stage >result &&
	grep "init.t" result
'

test_done
