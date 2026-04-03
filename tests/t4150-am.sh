#!/bin/sh

test_description='git am running'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# Use real git for setup operations that need features grit doesn't have yet
REAL_GIT=/usr/bin/git

# ── extra helpers not in our test-lib ────────────────────────────────

test_cmp_rev () {
	local a b
	a=$(git rev-parse "$1") &&
	b=$(git rev-parse "$2") &&
	test "$a" = "$b"
}

qz_to_tab_space () {
	tr Q '\t'
}

append_cr () {
	sed -e 's/$/\r/'
}

test_hook () {
	local hook_name="$1"
	mkdir -p .git/hooks &&
	cat > ".git/hooks/$hook_name" &&
	chmod +x ".git/hooks/$hook_name"
}

# ── init repo ────────────────────────────────────────────────────────

test_expect_success 'init repo for am tests' '
	git init &&
	git config user.name "C O Mitter" &&
	git config user.email "committer@example.com"
'

# ── setup: messages ──────────────────────────────────────────────────

test_expect_success 'setup: messages' '
	cat >msg <<-\EOF &&
	second

	Lorem ipsum dolor sit amet, consectetuer sadipscing elitr, sed diam nonumy
	eirmod tempor invidunt ut labore et dolore magna aliquyam erat, sed diam
	voluptua. At vero eos et accusam et justo duo dolores et ea rebum. Stet clita
	kasd gubergren, no sea takimata sanctus est Lorem ipsum dolor sit amet. Lorem
	ipsum dolor sit amet, consetetur sadipscing elitr, sed diam nonumy eirmod
	tempor invidunt ut labore et dolore magna aliquyam erat, sed diam voluptua. At
	vero eos et accusam et justo duo dolores et ea rebum.

	EOF
	qz_to_tab_space <<-\EOF >>msg &&
	QDuis autem vel eum iriure dolor in hendrerit in vulputate velit
	Qesse molestie consequat, vel illum dolore eu feugiat nulla facilisis
	Qat vero eros et accumsan et iusto odio dignissim qui blandit
	Qpraesent luptatum zzril delenit augue duis dolore te feugait nulla
	Qfacilisi.
	EOF
	cat >>msg <<-\EOF &&

	Lorem ipsum dolor sit amet,
	consectetuer adipiscing elit, sed diam nonummy nibh euismod tincidunt ut
	laoreet dolore magna aliquam erat volutpat.

	  git
	  ---
	  +++

	Ut wisi enim ad minim veniam, quis nostrud exerci tation ullamcorper suscipit
	lobortis nisl ut aliquip ex ea commodo consequat. Duis autem vel eum iriure
	dolor in hendrerit in vulputate velit esse molestie consequat, vel illum
	dolore eu feugiat nulla facilisis at vero eros et accumsan et iusto odio
	dignissim qui blandit praesent luptatum zzril delenit augue duis dolore te
	feugait nulla facilisi.

	Reported-by: A N Other <a.n.other@example.com>
	EOF

	cat >failmail <<-\EOF &&
	From foo@example.com Fri May 23 10:43:49 2008
	From:	foo@example.com
	To:	bar@example.com
	Subject: Re: [RFC/PATCH] git-foo.sh
	Date:	Fri, 23 May 2008 05:23:42 +0200

	Sometimes we have to find out that there'\''s nothing left.

	EOF

	cat >pine <<-\EOF &&
	From MAILER-DAEMON Fri May 23 10:43:49 2008
	Date: 23 May 2008 05:23:42 +0200
	From: Mail System Internal Data <MAILER-DAEMON@example.com>
	Subject: DON'\''T DELETE THIS MESSAGE -- FOLDER INTERNAL DATA
	Message-ID: <foo-0001@example.com>

	This text is part of the internal format of your mail folder, and is not
	a real message.  It is created automatically by the mail system software.
	If deleted, important folder data will be lost, and it will be re-created
	with the data reset to initial values.

	EOF

	cat >msg-without-scissors-line <<-\EOF &&
	Test that git-am --scissors cuts at the scissors line

	This line should be included in the commit message.
	EOF

	printf "Subject: " >subject-prefix &&

	cat - subject-prefix msg-without-scissors-line >msg-with-scissors-line <<-\EOF
	This line should not be included in the commit message with --scissors enabled.

	 - - >8 - - remove everything above this line - - >8 - -

	EOF
'

test_expect_success setup '
	echo hello >file &&
	git add file &&
	test_tick &&
	git commit -m first &&
	git tag first &&

	echo world >>file &&
	git add file &&
	test_tick &&
	git commit -F msg &&
	git tag second &&

	git format-patch --stdout first >patch1 &&
	{
		echo "Message-ID: <1226501681-24923-1-git-send-email-bda@mnsspb.ru>" &&
		echo "X-Fake-Field: Line One" &&
		echo "X-Fake-Field: Line Two" &&
		echo "X-Fake-Field: Line Three" &&
		git format-patch --stdout first | sed -e "1d"
	} > patch1.eml &&
	{
		echo "X-Fake-Field: Line One" &&
		echo "X-Fake-Field: Line Two" &&
		echo "X-Fake-Field: Line Three" &&
		git format-patch --stdout first | sed -e "1d"
	} | append_cr >patch1-crlf.eml &&
	{
		printf "%255s\\n" "" &&
		echo "X-Fake-Field: Line One" &&
		echo "X-Fake-Field: Line Two" &&
		echo "X-Fake-Field: Line Three" &&
		git format-patch --stdout first | sed -e "1d"
	} > patch1-ws.eml &&

	echo file >file &&
	git add file &&
	git commit -F msg-without-scissors-line &&
	git tag expected-for-scissors &&
	git reset --hard HEAD^ &&

	echo file >file &&
	git add file &&
	git commit -F msg-with-scissors-line &&
	git tag expected-for-no-scissors &&
	$REAL_GIT format-patch --stdout expected-for-no-scissors^ >patch-with-scissors-line.eml &&
	git reset --hard HEAD^ &&

	sed -n -e "3,\$p" msg >file &&
	git add file &&
	test_tick &&
	git commit -m third &&

	git format-patch --stdout first >patch2 &&

	git checkout -b lorem &&
	sed -n -e "11,\$p" msg >file &&
	head -n 9 msg >>file &&
	test_tick &&
	git commit -a -m "moved stuff" &&

	echo goodbye >another &&
	git add another &&
	test_tick &&
	git commit -m "added another file" &&

	$REAL_GIT format-patch --stdout main >lorem-move.patch &&
	$REAL_GIT format-patch --no-prefix --stdout main >lorem-zero.patch &&

	git checkout -b rename &&
	git mv file renamed &&
	git commit -m "renamed a file" &&

	$REAL_GIT format-patch -M --stdout lorem >rename.patch &&

	git reset --soft lorem^ &&
	git commit -m "renamed a file and added another" &&

	$REAL_GIT format-patch -M --stdout lorem^ >rename-add.patch &&

	git checkout -b empty-commit &&
	git commit -m "empty commit" --allow-empty &&

	: >empty.patch &&
	$REAL_GIT format-patch --always --stdout empty-commit^ >empty-commit.patch &&

	# reset time
	sane_unset test_tick &&
	test_tick
'

test_expect_success 'am applies patch correctly' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_tick &&
	git am <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	git diff --exit-code second &&
	test "$(git rev-parse second)" = "$(git rev-parse HEAD)" &&
	test "$(git rev-parse second^)" = "$(git rev-parse HEAD^)"
'

test_expect_success 'am applies patch e-mail not in a mbox' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am patch1.eml &&
	test_path_is_missing .git/rebase-apply &&
	git diff --exit-code second &&
	test "$(git rev-parse second)" = "$(git rev-parse HEAD)" &&
	test "$(git rev-parse second^)" = "$(git rev-parse HEAD^)"
'

test_expect_success 'am applies patch e-mail not in a mbox with CRLF' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am patch1-crlf.eml &&
	test_path_is_missing .git/rebase-apply &&
	git diff --exit-code second &&
	test "$(git rev-parse second)" = "$(git rev-parse HEAD)" &&
	test "$(git rev-parse second^)" = "$(git rev-parse HEAD^)"
'

test_expect_success 'am applies patch e-mail with preceding whitespace' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am patch1-ws.eml &&
	test_path_is_missing .git/rebase-apply &&
	git diff --exit-code second &&
	test "$(git rev-parse second)" = "$(git rev-parse HEAD)" &&
	test "$(git rev-parse second^)" = "$(git rev-parse HEAD^)"
'

test_expect_failure 'am applies stgit patch' '
	false
'

test_expect_failure 'am --patch-format=stgit applies stgit patch' '
	false
'

test_expect_failure 'am applies stgit series' '
	false
'

test_expect_failure 'am applies hg patch' '
	false
'

test_expect_failure 'am --patch-format=hg applies hg patch' '
	false
'

test_expect_success 'am with applypatch-msg hook' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook applypatch-msg <<-\EOF &&
	echo "hook ran" >"$(git rev-parse --git-dir)/apply-hook-ran"
	EOF
	git am <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	test -f .git/apply-hook-ran &&
	rm -f .git/apply-hook-ran &&
	rm -f .git/hooks/applypatch-msg
'

test_expect_success 'am with failing applypatch-msg hook' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook applypatch-msg <<-\EOF &&
	exit 1
	EOF
	test_must_fail git am <patch1 &&
	rm -f .git/hooks/applypatch-msg &&
	git am --abort
'

test_expect_success 'am with failing applypatch-msg hook (no verify)' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook applypatch-msg <<-\EOF &&
	exit 1
	EOF
	git am --no-verify <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	rm -f .git/hooks/applypatch-msg
'

test_expect_success 'am with pre-applypatch hook' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook pre-applypatch <<-\EOF &&
	echo "pre-hook ran" >"$(git rev-parse --git-dir)/pre-hook-ran"
	EOF
	git am <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	test -f .git/pre-hook-ran &&
	rm -f .git/pre-hook-ran &&
	rm -f .git/hooks/pre-applypatch
'

test_expect_success 'am with failing pre-applypatch hook' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook pre-applypatch <<-\EOF &&
	exit 1
	EOF
	test_must_fail git am <patch1 &&
	rm -f .git/hooks/pre-applypatch &&
	git am --abort
'

test_expect_success 'am with failing pre-applypatch hook (no verify)' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook pre-applypatch <<-\EOF &&
	exit 1
	EOF
	git am --no-verify <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	rm -f .git/hooks/pre-applypatch
'

test_expect_success 'am with post-applypatch hook' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook post-applypatch <<-\EOF &&
	echo "post-hook ran" >"$(git rev-parse --git-dir)/post-hook-ran"
	EOF
	git am <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	test -f .git/post-hook-ran &&
	rm -f .git/post-hook-ran &&
	rm -f .git/hooks/post-applypatch
'

test_expect_success 'am with failing post-applypatch hook' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_hook post-applypatch <<-\EOF &&
	exit 1
	EOF
	git am <patch1 &&
	test_path_is_missing .git/rebase-apply &&
	rm -f .git/hooks/post-applypatch
'

test_expect_success 'am --scissors cuts the message at the scissors line' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --scissors patch-with-scissors-line.eml &&
	test_path_is_missing .git/rebase-apply &&
	git log --format=%B -1 HEAD >actual &&
	grep "should be included" actual &&
	! grep "should not be included" actual
'

test_expect_success 'am --no-scissors overrides mailinfo.scissors' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --no-scissors patch-with-scissors-line.eml &&
	test_path_is_missing .git/rebase-apply &&
	git log --format=%B -1 HEAD >actual &&
	grep "should not be included" actual
'

test_expect_success 'setup: new author and committer' '
	GIT_AUTHOR_NAME="Another Thor" &&
	GIT_AUTHOR_EMAIL="a.thor@example.com" &&
	GIT_COMMITTER_NAME="Co M Miter" &&
	GIT_COMMITTER_EMAIL="c.miter@example.com" &&
	export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL &&
	cat > "$TRASH_DIRECTORY/.test_env" <<-ENVEOF
	GIT_AUTHOR_NAME="Another Thor"
	GIT_AUTHOR_EMAIL="a.thor@example.com"
	GIT_COMMITTER_NAME="Co M Miter"
	GIT_COMMITTER_EMAIL="c.miter@example.com"
	export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL
	ENVEOF
'

compare () {
	a=$(git cat-file commit "$2" | grep "^$1 ") &&
	b=$(git cat-file commit "$3" | grep "^$1 ") &&
	test "$a" = "$b"
}

test_expect_success 'am changes committer and keeps author' '
	test_tick &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	GIT_AUTHOR_NAME="Another Thor" &&
	GIT_AUTHOR_EMAIL="a.thor@example.com" &&
	GIT_COMMITTER_NAME="Co M Miter" &&
	GIT_COMMITTER_EMAIL="c.miter@example.com" &&
	export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL &&
	git am patch2 &&
	test_path_is_missing .git/rebase-apply &&
	test "$(git rev-parse main^^)" = "$(git rev-parse HEAD^^)" &&
	git diff --exit-code main HEAD &&
	git diff --exit-code main^ HEAD^ &&
	compare author main HEAD &&
	compare author main^ HEAD^ &&
	test "$GIT_COMMITTER_NAME <$GIT_COMMITTER_EMAIL>" = \
	     "$(git log -n 1 --pretty=format:"%cn <%ce>" HEAD)"
'

test_expect_success 'am --signoff adds Signed-off-by: line' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --signoff <patch2 &&
	git cat-file commit HEAD >actual &&
	grep "Signed-off-by:" actual
'

test_expect_success 'am stays in branch' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout -b stay-in-branch first &&
	git am --signoff <patch2 &&
	test "refs/heads/stay-in-branch" = "$(git symbolic-ref HEAD)"
'

test_expect_success 'am --signoff does not add Signed-off-by: line if already there' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --signoff <patch2 &&
	git cat-file commit HEAD >actual &&
	test $(grep -c "Signed-off-by:" actual) -eq 1
'

test_expect_success 'am --signoff adds Signed-off-by: if another author is preset' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	GIT_COMMITTER_NAME="New Committer" \
	GIT_COMMITTER_EMAIL="new@example.com" \
	git am --signoff <patch2 &&
	git cat-file commit HEAD >actual &&
	grep "Signed-off-by: New Committer <new@example.com>" actual
'

test_expect_success 'am --signoff duplicates Signed-off-by: if it is not the last one' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --signoff <patch2 &&
	git cat-file commit HEAD >actual &&
	grep "Signed-off-by:" actual
'

test_expect_success 'am without --keep removes Re: and [PATCH] stuff' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am <patch2 &&
	git log --format=%s -1 HEAD >actual &&
	! grep "\[PATCH" actual
'

test_expect_success 'am --keep really keeps the subject' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --keep <patch2 &&
	git log --format=%s -1 HEAD >actual &&
	grep "\[PATCH" actual
'

test_expect_success 'am --keep-non-patch really keeps the non-patch part' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	git am --keep-non-patch <patch2 &&
	test_path_is_missing .git/rebase-apply
'

test_expect_failure 'setup am -3' '
	false
'

test_expect_failure 'am -3 falls back to 3-way merge' '
	false
'

test_expect_failure 'am -3 -p0 can read --no-prefix patch' '
	false
'

test_expect_failure 'am with config am.threeWay falls back to 3-way merge' '
	false
'

test_expect_failure 'am with config am.threeWay overridden by --no-3way' '
	false
'

test_expect_success 'am can rename a file' '
	grep "^rename from" rename.patch &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout lorem^0 &&
	git am rename.patch &&
	test_path_is_missing .git/rebase-apply &&
	git update-index --refresh &&
	git diff --exit-code rename
'

test_expect_failure 'am -3 can rename a file' '
	false
'

test_expect_failure 'am -3 can rename a file after falling back to 3-way merge' '
	false
'

test_expect_failure 'am -3 -q is quiet' '
	false
'

test_expect_success 'am pauses on conflict' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_must_fail git am lorem-move.patch &&
	test -d .git/rebase-apply
'

test_expect_success 'am --show-current-patch' '
	git am --show-current-patch >actual &&
	test -s actual
'

test_expect_success 'am --show-current-patch=raw' '
	git am --show-current-patch=raw >actual &&
	test -s actual
'

test_expect_success 'am --show-current-patch=diff' '
	git am --show-current-patch=diff >actual &&
	test -s actual
'

test_expect_success 'am accepts repeated --show-current-patch' '
	git am --show-current-patch=raw >actual &&
	test -s actual
'

test_expect_success 'am detects incompatible --show-current-patch' '
	test_must_fail git am --show-current-patch=invalid 2>err
'

test_expect_success 'am --skip works' '
	echo goodbye >expected &&
	git am --skip &&
	test_path_is_missing .git/rebase-apply &&
	git diff --exit-code first -- file &&
	test_cmp expected another
'

test_expect_success 'am --abort removes a stray directory' '
	mkdir -p .git/rebase-apply &&
	touch .git/rebase-apply/applying &&
	git am --abort &&
	test_path_is_missing .git/rebase-apply
'

test_expect_success 'am refuses patches when paused' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&

	test_must_fail git am lorem-move.patch &&
	test_path_is_dir .git/rebase-apply &&
	test_cmp_rev first HEAD &&

	test_must_fail git am <lorem-move.patch &&
	test_path_is_dir .git/rebase-apply &&
	test_cmp_rev first HEAD
'

test_expect_success 'am --resolved works' '
	echo goodbye >expected &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_must_fail git am lorem-move.patch &&
	test -d .git/rebase-apply &&
	echo resolved >>file &&
	git add file &&
	git am --continue &&
	test_path_is_missing .git/rebase-apply &&
	test_cmp expected another
'

test_expect_success 'am --resolved fails if index has no changes' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_must_fail git am lorem-move.patch &&
	test_path_is_dir .git/rebase-apply &&
	test_cmp_rev first HEAD &&
	test_must_fail git am --continue &&
	test_path_is_dir .git/rebase-apply &&
	test_cmp_rev first HEAD
'

test_expect_success 'am --resolved fails if index has unmerged entries' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_must_fail git am lorem-move.patch &&
	test_path_is_dir .git/rebase-apply &&
	test_must_fail git am --continue 2>err &&
	git am --abort
'

test_expect_success 'am takes patches from a Pine mailbox' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	cat pine patch1 | git am &&
	test_path_is_missing .git/rebase-apply &&
	git diff --exit-code main^ HEAD
'

test_expect_success 'am fails on mail without patch' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	test_must_fail git am <failmail &&
	git am --abort &&
	test_path_is_missing .git/rebase-apply
'

test_expect_success 'am fails on empty patch' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	echo "---" >>failmail &&
	test_must_fail git am <failmail &&
	git am --skip &&
	test_path_is_missing .git/rebase-apply
'

test_expect_success 'am works from stdin in subdirectory' '
	rm -fr subdir &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	(
		mkdir -p subdir &&
		cd subdir &&
		git am <../patch1
	) &&
	git diff --exit-code second
'

test_expect_success 'am works from file (relative path given) in subdirectory' '
	rm -fr subdir &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	(
		mkdir -p subdir &&
		cd subdir &&
		git am ../patch1
	) &&
	git diff --exit-code second
'

test_expect_success 'am works from file (absolute path given) in subdirectory' '
	rm -fr subdir &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	P=$(pwd) &&
	(
		mkdir -p subdir &&
		cd subdir &&
		git am "$P/patch1"
	) &&
	git diff --exit-code second
'

test_expect_success 'am --committer-date-is-author-date' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_tick &&
	git am --committer-date-is-author-date patch1 &&
	git cat-file commit HEAD | sed -e "/^\$/q" >head1 &&
	sed -ne "/^author /s/.*> //p" head1 >at &&
	sed -ne "/^committer /s/.*> //p" head1 >ct &&
	test_cmp at ct
'

test_expect_success 'am without --committer-date-is-author-date' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_tick &&
	git am patch1 &&
	git cat-file commit HEAD | sed -e "/^\$/q" >head1 &&
	sed -ne "/^author /s/.*> //p" head1 >at &&
	sed -ne "/^committer /s/.*> //p" head1 >ct &&
	! test_cmp at ct
'

test_expect_failure 'am --ignore-date' '
	false
'

test_expect_success 'am into an unborn branch' '
	git rev-parse first^{tree} >expected &&
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	rm -fr subdir &&
	mkdir subdir &&
	$REAL_GIT format-patch --numbered-files -o subdir -1 first &&
	(
		cd subdir &&
		git init &&
		git config user.name "Test User" &&
		git config user.email "test@example.com" &&
		git am 1
	) &&
	(
		cd subdir &&
		git rev-parse HEAD^{tree} >../actual
	) &&
	test_cmp expected actual
'

test_expect_success 'am -q is quiet' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	test_tick &&
	git am -q <patch1 >output.out 2>&1 &&
	test_must_be_empty output.out
'

test_expect_success 'am empty-file does not infloop' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	touch empty-file &&
	test_tick &&
	test_must_fail git am empty-file 2>actual &&
	echo "Patch format detection failed." >expected &&
	test_cmp expected actual
'

test_expect_failure 'am --message-id really adds the message id' '
	false
'

test_expect_failure 'am.messageid really adds the message id' '
	false
'

test_expect_failure 'am --message-id -s signs off after the message id' '
	false
'

test_expect_failure 'am -3 works with rerere' '
	false
'

test_expect_failure 'am -s unexpected trailer block' '
	false
'

test_expect_failure 'am --patch-format=mboxrd handles mboxrd' '
	false
'

test_expect_failure 'am works with multi-line in-body headers' '
	false
'

test_expect_success 'am --quit keeps HEAD where it is' '
	mkdir -p .git/rebase-apply &&
	touch .git/rebase-apply/applying &&
	>.git/rebase-apply/last &&
	>.git/rebase-apply/next &&
	git rev-parse HEAD >expected &&
	git am --abort &&
	test_path_is_missing .git/rebase-apply &&
	git rev-parse HEAD >actual &&
	test_cmp expected actual
'

test_expect_failure 'am and .gitattibutes' '
	false
'

test_expect_failure 'apply binary blob in partial clone' '
	false
'

test_expect_failure 'an empty input file is error regardless of --empty option' '
	false
'

test_expect_failure 'invalid when passing the --empty option alone' '
	false
'

test_expect_failure 'a message without a patch is an error (default)' '
	false
'

test_expect_failure 'a message without a patch is an error where an explicit "--empty=stop" is given' '
	false
'

test_expect_failure 'a message without a patch will be skipped when "--empty=drop" is given' '
	false
'

test_expect_failure 'record as an empty commit when meeting e-mail message that lacks a patch' '
	false
'

test_expect_failure 'skip an empty patch in the middle of an am session' '
	false
'

test_expect_failure 'record an empty patch as an empty commit in the middle of an am session' '
	false
'

test_expect_failure 'create an non-empty commit when the index IS changed though "--allow-empty" is given' '
	false
'

test_expect_failure 'cannot create empty commits when there is a clean index due to merge conflicts' '
	false
'

test_expect_failure 'cannot create empty commits when there is unmerged index due to merge conflicts' '
	false
'

test_expect_success 'am fails if index is dirty' '
	rm -fr .git/rebase-apply &&
	git reset --hard &&
	git checkout first &&
	echo dirtyfile >dirtyfile &&
	git add dirtyfile &&
	test_must_fail git am patch1 &&
	test_path_is_dir .git/rebase-apply &&
	test_cmp_rev first HEAD &&
	rm -f dirtyfile &&
	git am --abort
'

test_done
