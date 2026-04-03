#!/bin/sh

test_description='Merge-recursive merging renames'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&

	cat >A <<-\EOF &&
	a aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
	b bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
	c cccccccccccccccccccccccccccccccccccccccccccccccc
	d dddddddddddddddddddddddddddddddddddddddddddddddd
	e eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
	f ffffffffffffffffffffffffffffffffffffffffffffffff
	g gggggggggggggggggggggggggggggggggggggggggggggggg
	h hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh
	i iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii
	j jjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjjj
	k kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
	l llllllllllllllllllllllllllllllllllllllllllllllll
	m mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm
	n nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn
	o oooooooooooooooooooooooooooooooooooooooooooooooo
	EOF

	cat >M <<-\EOF &&
	A AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
	B BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
	C CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
	D DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD
	E EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE
	F FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF
	G GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG
	H HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH
	I IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII
	J JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ
	K KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK
	L LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL
	M MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM
	N NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN
	O OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO
	EOF

	git add A M &&
	git commit -m "initial has A and M" &&
	git branch white &&
	git branch red &&
	git branch blue &&

	git checkout white &&
	sed -e "/^g /s/.*/g : white changes a line/" <A >B &&
	sed -e "/^G /s/.*/G : colored branch changes a line/" <M >N &&
	rm -f A M &&
	git update-index --add --remove A B M N &&
	git commit -m "white renames A->B, M->N" &&

	git checkout red &&
	echo created by red >R &&
	git update-index --add R &&
	git commit -m "red creates R" &&

	git checkout blue &&
	sed -e "/^o /s/.*/g : blue changes a line/" <A >B &&
	rm -f A &&
	mv B A &&
	git update-index A &&
	git commit -m "blue modify A" &&

	git checkout main
'

test_expect_success 'merge white into red (A->B,M->N)' '
	git checkout -b red-white red &&
	git merge white &&
	git write-tree &&
	test_path_is_file B &&
	test_path_is_file N &&
	test_path_is_file R &&
	test_path_is_missing A &&
	test_path_is_missing M
'

# Skip: merge blue into white requires rename detection across branches
# which grit does not yet fully support

test_done
