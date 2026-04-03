#!/bin/sh

test_description='clone and fetch with project files'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo data >mainfile &&
	git add mainfile &&
	test_tick &&
	git commit -m "initial commit"
'

test_expect_success 'clone works' '
	git clone . cloned &&
	(git rev-parse HEAD && git ls-files -s) >expected &&
	(
		cd cloned &&
		(git rev-parse HEAD && git ls-files -s) >../actual
	) &&
	test_cmp expected actual
'

test_expect_success 'advance and fetch' '
	echo more >mainfile &&
	git add mainfile &&
	test_tick &&
	git commit -m "second commit" &&
	(
		cd cloned &&
		git pull &&
		echo more >expect &&
		test_cmp expect mainfile
	)
'

test_done
