#!/bin/sh

test_description='ls-tree --format'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	mkdir dir &&
	echo "sub content" >dir/sub-file.t &&
	git add dir/sub-file.t &&
	test_tick &&
	git commit -m "dir/sub-file" &&
	echo "top content" >top-file.t &&
	git add top-file.t &&
	test_tick &&
	git commit -m "top-file"
'

test_expect_success "ls-tree '--format=%(path)' is like --name-only" '
	git ls-tree --name-only -r HEAD >expect &&
	git ls-tree --format="%(path)" -r HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree '--format=<default>' matches default output" '
	git ls-tree -r HEAD >expect &&
	git ls-tree --format="%(objectmode) %(objecttype) %(objectname)%x09%(path)" -r HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree --format='%(path) %(path) %(path)' HEAD top-file.t" '
	git ls-tree --format="%(path) %(path) %(path)" HEAD top-file.t >actual &&
	echo "top-file.t top-file.t top-file.t" >expect &&
	test_cmp expect actual
'

test_expect_success "ls-tree '--format=<default>' on optimized v.s. non-optimized path" '
	git ls-tree --format="%(objectmode) %(objecttype) %(objectname)%x09%(path)" -r HEAD >expect &&
	git ls-tree --format="> %(objectmode) %(objecttype) %(objectname)%x09%(path)" -r HEAD >actual.raw &&
	sed "s/^> //" >actual <actual.raw &&
	test_cmp expect actual
'

test_done
