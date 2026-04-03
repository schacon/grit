#!/bin/sh

test_description='ls-tree output'

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

test_expect_success "ls-tree default output" '
	HEAD_dir=$(git rev-parse HEAD:dir) &&
	HEAD_top_file=$(git rev-parse HEAD:top-file.t) &&
	cat >expect <<-EOF &&
	040000 tree $HEAD_dir	dir
	100644 blob $HEAD_top_file	top-file.t
	EOF
	git ls-tree HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree -d output" '
	HEAD_dir=$(git rev-parse HEAD:dir) &&
	cat >expect <<-EOF &&
	040000 tree $HEAD_dir	dir
	EOF
	git ls-tree -d HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree -r output" '
	HEAD_dir_sub_file=$(git rev-parse HEAD:dir/sub-file.t) &&
	HEAD_top_file=$(git rev-parse HEAD:top-file.t) &&
	cat >expect <<-EOF &&
	100644 blob $HEAD_dir_sub_file	dir/sub-file.t
	100644 blob $HEAD_top_file	top-file.t
	EOF
	git ls-tree -r HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree -t output" '
	HEAD_dir=$(git rev-parse HEAD:dir) &&
	HEAD_top_file=$(git rev-parse HEAD:top-file.t) &&
	cat >expect <<-EOF &&
	040000 tree $HEAD_dir	dir
	100644 blob $HEAD_top_file	top-file.t
	EOF
	git ls-tree -t HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree -r -t output" '
	HEAD_dir=$(git rev-parse HEAD:dir) &&
	HEAD_dir_sub_file=$(git rev-parse HEAD:dir/sub-file.t) &&
	HEAD_top_file=$(git rev-parse HEAD:top-file.t) &&
	cat >expect <<-EOF &&
	040000 tree $HEAD_dir	dir
	100644 blob $HEAD_dir_sub_file	dir/sub-file.t
	100644 blob $HEAD_top_file	top-file.t
	EOF
	git ls-tree -r -t HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree --name-only output" '
	cat >expect <<-EOF &&
	dir
	top-file.t
	EOF
	git ls-tree --name-only HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree --name-only -r output" '
	cat >expect <<-EOF &&
	dir/sub-file.t
	top-file.t
	EOF
	git ls-tree --name-only -r HEAD >actual &&
	test_cmp expect actual
'

test_expect_success "ls-tree --name-only -d output" '
	cat >expect <<-EOF &&
	dir
	EOF
	git ls-tree --name-only -d HEAD >actual &&
	test_cmp expect actual
'

test_done
