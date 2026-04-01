git_show_ref_exists=${git_show_ref_exists:-git show-ref --exists}

test_expect_success 'setup' '
	gust init repo &&
	cd repo &&
	tree=$(git write-tree) &&
	commit=$(echo base | git commit-tree "$tree") &&
	gust update-ref refs/heads/master "$commit" &&
	gust update-ref refs/heads/main "$commit" &&
	gust update-ref refs/heads/side "$commit"
'

test_expect_success '--exists with existing reference' '
	cd repo &&
	${git_show_ref_exists} refs/heads/side
'

test_expect_success '--exists with missing reference' '
	cd repo &&
	test_must_fail ${git_show_ref_exists} refs/heads/does-not-exist
'

test_expect_success '--exists does not use DWIM' '
	cd repo &&
	test_must_fail ${git_show_ref_exists} side 2>err &&
	grep "reference does not exist" err
'

test_expect_success '--exists with HEAD' '
	cd repo &&
	${git_show_ref_exists} HEAD
'

test_expect_success '--exists with arbitrary symref' '
	cd repo &&
	git symbolic-ref refs/symref refs/heads/side &&
	${git_show_ref_exists} refs/symref
'

test_expect_success '--exists with dangling symref' '
	cd repo &&
	git symbolic-ref refs/heads/dangling refs/heads/does-not-exist &&
	${git_show_ref_exists} refs/heads/dangling
'

test_expect_success '--exists with directory reports missing ref' '
	cd repo &&
	test_must_fail ${git_show_ref_exists} refs/heads 2>err &&
	grep "reference does not exist" err
'

