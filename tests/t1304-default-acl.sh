#!/bin/sh
# Test default ACL/permissions on files created by grit.

test_description='grit default ACL and permissions on created files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Helper: get octal permissions
octal_perms () {
	stat -c '%a' "$1" 2>/dev/null || stat -f '%Lp' "$1" 2>/dev/null
}

# Helper: assert file perms are sane (644 or 664)
assert_file_perms () {
	p=$(octal_perms "$1") &&
	case "$p" in
	644|664) true ;;
	*) echo "unexpected perms for $1: $p"; false ;;
	esac
}

# Helper: assert dir perms are sane (755 or 775)
assert_dir_perms () {
	p=$(octal_perms "$1") &&
	case "$p" in
	755|775) true ;;
	*) echo "unexpected perms for $1: $p"; false ;;
	esac
}

###########################################################################
# Section 1: init creates sane permissions
###########################################################################

test_expect_success 'init: .git directory has correct permissions' '
	grit init perms-repo &&
	cd perms-repo &&
	assert_dir_perms .git
'

test_expect_success 'init: .git/HEAD is readable' '
	cd perms-repo &&
	test -r .git/HEAD
'

test_expect_success 'init: .git/HEAD has sane permissions' '
	cd perms-repo &&
	assert_file_perms .git/HEAD
'

test_expect_success 'init: .git/config has sane permissions' '
	cd perms-repo &&
	assert_file_perms .git/config
'

test_expect_success 'init: .git/objects is a directory with sane permissions' '
	cd perms-repo &&
	assert_dir_perms .git/objects
'

test_expect_success 'init: .git/refs is a directory with sane permissions' '
	cd perms-repo &&
	assert_dir_perms .git/refs
'

test_expect_success 'init: .git/objects/pack has sane permissions' '
	cd perms-repo &&
	assert_dir_perms .git/objects/pack
'

test_expect_success 'init: .git/objects/info has sane permissions' '
	cd perms-repo &&
	assert_dir_perms .git/objects/info
'

###########################################################################
# Section 2: objects and index permissions
###########################################################################

test_expect_success 'setup: configure and create objects' '
	cd perms-repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test User" &&
	echo "hello" >file.txt &&
	grit add file.txt &&
	grit commit -m "first commit"
'

test_expect_success 'loose objects are not world-writable' '
	cd perms-repo &&
	find .git/objects -type f -name "??" -prune -o -type f -print | while read f; do
		p=$(octal_perms "$f")
		case "$p" in
		*[2367][2367]) echo "FAIL: $f has perms $p (world/group writable)"; exit 1 ;;
		esac
	done
'

test_expect_success 'loose objects are readable by owner' '
	cd perms-repo &&
	find .git/objects -type f -not -path "*/info/*" -not -path "*/pack/*" | head -5 | while read f; do
		test -r "$f" || { echo "FAIL: $f not readable"; exit 1; }
	done
'

test_expect_success 'index file has sane permissions' '
	cd perms-repo &&
	test -f .git/index &&
	assert_file_perms .git/index
'

test_expect_success 'COMMIT_EDITMSG has sane permissions if it exists' '
	cd perms-repo &&
	if test -f .git/COMMIT_EDITMSG; then
		assert_file_perms .git/COMMIT_EDITMSG
	fi
'

###########################################################################
# Section 3: refs and logs permissions
###########################################################################

test_expect_success 'refs/heads/master file has sane permissions' '
	cd perms-repo &&
	if test -f .git/refs/heads/master; then
		assert_file_perms .git/refs/heads/master
	fi
'

test_expect_success 'update-ref creates ref with sane permissions' '
	cd perms-repo &&
	grit update-ref refs/heads/new-ref HEAD &&
	if test -f .git/refs/heads/new-ref; then
		assert_file_perms .git/refs/heads/new-ref
	fi
'

test_expect_success 'tag ref has sane permissions' '
	cd perms-repo &&
	grit tag v1.0 &&
	if test -f .git/refs/tags/v1.0; then
		assert_file_perms .git/refs/tags/v1.0
	fi
'

###########################################################################
# Section 4: checkout and working tree permissions
###########################################################################

test_expect_success 'checked out files are not executable by default' '
	cd perms-repo &&
	assert_file_perms file.txt
'

test_expect_success 'second commit preserves file permissions pattern' '
	cd perms-repo &&
	echo "world" >file2.txt &&
	grit add file2.txt &&
	grit commit -m "second" &&
	assert_file_perms file2.txt
'

test_expect_success 'checkout restores files with correct permissions' '
	cd perms-repo &&
	rm file.txt &&
	grit checkout -- file.txt &&
	test -f file.txt &&
	assert_file_perms file.txt
'

###########################################################################
# Section 5: reinit and umask interaction
###########################################################################

test_expect_success 'reinit preserves existing permissions' '
	cd perms-repo &&
	old_head_perms=$(octal_perms .git/HEAD) &&
	grit init . &&
	new_head_perms=$(octal_perms .git/HEAD) &&
	test "$old_head_perms" = "$new_head_perms"
'

test_expect_success 'init in new directory with restrictive umask' '
	old_umask=$(umask) &&
	umask 077 &&
	grit init restrictive-repo &&
	umask $old_umask &&
	test -d restrictive-repo/.git &&
	test -r restrictive-repo/.git/HEAD
'

test_expect_success 'objects created under restrictive umask are owner-readable' '
	cd restrictive-repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test" &&
	echo "secret" >secret.txt &&
	grit add secret.txt &&
	old_umask=$(umask) &&
	umask 077 &&
	grit commit -m "secret commit" &&
	umask $old_umask &&
	find .git/objects -type f -not -path "*/info/*" -not -path "*/pack/*" | head -3 | while read f; do
		test -r "$f" || { echo "FAIL: $f not readable"; exit 1; }
	done
'

test_expect_success 'config file is owner-readable under restrictive umask' '
	cd restrictive-repo &&
	test -r .git/config
'

test_expect_success 'new branch ref inherits sane permissions' '
	cd perms-repo &&
	grit checkout master &&
	grit checkout -b perms-branch &&
	if test -f .git/refs/heads/perms-branch; then
		assert_file_perms .git/refs/heads/perms-branch
	fi
'

test_expect_success 'multiple files added all have consistent permissions' '
	cd perms-repo &&
	grit checkout master &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test User" &&
	for i in 1 2 3 4 5; do
		echo "content $i" >multi_$i.txt
	done &&
	grit add multi_1.txt multi_2.txt multi_3.txt multi_4.txt multi_5.txt &&
	grit commit -m "multi files" &&
	p1=$(octal_perms multi_1.txt) &&
	p2=$(octal_perms multi_2.txt) &&
	p3=$(octal_perms multi_3.txt) &&
	test "$p1" = "$p2" &&
	test "$p2" = "$p3"
'

test_done
