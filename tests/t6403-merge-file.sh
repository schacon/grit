#!/bin/sh

test_description='three-way file merge: merge-file'

. ./test-lib.sh

test_expect_success 'setup' '
	cat >orig.txt <<-\EOF &&
	Dominus regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	EOF

	cat >new1.txt <<-\EOF &&
	Dominus regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam tu mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	cat >new2.txt <<-\EOF &&
	Dominus regit me, et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	EOF

	cat >new3.txt <<-\EOF &&
	DOMINUS regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	EOF

	cat >new4.txt <<-\EOF &&
	Dominus regit me, et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	EOF

	printf "propter nomen suum." >>new4.txt
'

test_expect_success 'merge with no changes' '
	cp orig.txt test.txt &&
	git merge-file test.txt orig.txt orig.txt &&
	test_cmp test.txt orig.txt
'

test_expect_success "merge without conflict" '
	cp new1.txt test.txt &&
	git merge-file test.txt orig.txt new2.txt
'

test_expect_success 'works in subdirectory' '
	mkdir -p dir &&
	cp new1.txt dir/a.txt &&
	cp orig.txt dir/o.txt &&
	cp new2.txt dir/b.txt &&
	( cd dir && git merge-file a.txt o.txt b.txt ) &&
	test_path_is_missing a.txt
'

test_expect_success "merge without conflict (--quiet)" '
	cp new1.txt test.txt &&
	git merge-file --quiet test.txt orig.txt new2.txt
'

test_expect_success "merge without conflict (missing LF at EOF, away from change in the other file)" '
	cp new4.txt test3.txt &&
	git merge-file --quiet test3.txt new2.txt new3.txt
'

test_expect_success "merge does not add LF away of change" '
	cat >expect.txt <<-\EOF &&
	DOMINUS regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	EOF
	printf "propter nomen suum." >>expect.txt &&

	test_cmp expect.txt test3.txt
'

test_expect_success "merge with conflicts" '
	cp test.txt backup.txt &&
	test_must_fail git merge-file test.txt orig.txt new3.txt
'

test_expect_success "expected conflict markers" '
	cat >expect.txt <<-\EOF &&
	<<<<<<< test.txt
	Dominus regit me, et nihil mihi deerit.
	=======
	DOMINUS regit me,
	et nihil mihi deerit.
	>>>>>>> new3.txt
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam tu mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	test_cmp expect.txt test.txt
'

test_expect_success "merge conflicting with --ours" '
	cp backup.txt test.txt &&

	cat >expect.txt <<-\EOF &&
	Dominus regit me, et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam tu mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	git merge-file --ours test.txt orig.txt new3.txt &&
	test_cmp expect.txt test.txt
'

test_expect_success "merge conflicting with --theirs" '
	cp backup.txt test.txt &&

	cat >expect.txt <<-\EOF &&
	DOMINUS regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam tu mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	git merge-file --theirs test.txt orig.txt new3.txt &&
	test_cmp expect.txt test.txt
'

test_expect_success "merge conflicting with --union" '
	cp backup.txt test.txt &&

	cat >expect.txt <<-\EOF &&
	Dominus regit me, et nihil mihi deerit.
	DOMINUS regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam tu mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	git merge-file --union test.txt orig.txt new3.txt &&
	test_cmp expect.txt test.txt
'

test_expect_success "merge with conflicts, using -L" '
	cp backup.txt test.txt &&
	test_must_fail git merge-file -L 1 -L 2 test.txt orig.txt new3.txt
'

test_expect_success "expected conflict markers, with -L" '
	cat >expect.txt <<-\EOF &&
	<<<<<<< 1
	Dominus regit me, et nihil mihi deerit.
	=======
	DOMINUS regit me,
	et nihil mihi deerit.
	>>>>>>> new3.txt
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam tu mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	test_cmp expect.txt test.txt
'

test_expect_success "conflict in removed tail" '
	sed "s/ tu / TU /" <new1.txt >new5.txt &&
	test_must_fail git merge-file -p orig.txt new1.txt new5.txt >out
'

test_expect_success "expected conflict markers" '
	cat >expect <<-\EOF &&
	Dominus regit me,
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	<<<<<<< orig.txt
	=======
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam TU mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	>>>>>>> new5.txt
	EOF

	test_cmp expect out
'

test_expect_success 'binary files cannot be merged' '
	printf "\000binary" >binary.bin &&
	test_must_fail git merge-file -p \
		orig.txt binary.bin new1.txt 2>merge.err &&
	grep "Cannot merge binary files" merge.err
'

test_expect_success '"diff3 -m" style output (1)' '
	sed -e "s/deerit.\$/deerit;/" -e "s/me;\$/me./" <new5.txt >new6.txt &&
	sed -e "s/deerit.\$/deerit,/" -e "s/me;\$/me,/" <new5.txt >new7.txt &&

	sed -e "s/deerit./&%%%%/" -e "s/locavit,/locavit;/" <new6.txt | tr % "\012" >new8.txt &&
	sed -e "s/deerit./&%%%%/" -e "s/locavit,/locavit --/" <new7.txt | tr % "\012" >new9.txt &&

	cat >expect <<-\EOF &&
	Dominus regit me,
	<<<<<<< new8.txt
	et nihil mihi deerit;




	In loco pascuae ibi me collocavit;
	super aquam refectionis educavit me.
	||||||| new5.txt
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	=======
	et nihil mihi deerit,




	In loco pascuae ibi me collocavit --
	super aquam refectionis educavit me,
	>>>>>>> new9.txt
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam TU mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	test_must_fail git merge-file -p --diff3 \
		new8.txt new5.txt new9.txt >actual &&
	test_cmp expect actual
'

test_expect_success 'marker size' '
	sed -e "s/deerit.\$/deerit;/" -e "s/me;\$/me./" <new5.txt >new6.txt &&
	sed -e "s/deerit.\$/deerit,/" -e "s/me;\$/me,/" <new5.txt >new7.txt &&
	sed -e "s/deerit./&%%%%/" -e "s/locavit,/locavit;/" <new6.txt | tr % "\012" >new8.txt &&
	sed -e "s/deerit./&%%%%/" -e "s/locavit,/locavit --/" <new7.txt | tr % "\012" >new9.txt &&

	cat >expect <<-\EOF &&
	Dominus regit me,
	<<<<<<<<<< new8.txt
	et nihil mihi deerit;




	In loco pascuae ibi me collocavit;
	super aquam refectionis educavit me.
	|||||||||| new5.txt
	et nihil mihi deerit.
	In loco pascuae ibi me collocavit,
	super aquam refectionis educavit me;
	==========
	et nihil mihi deerit,




	In loco pascuae ibi me collocavit --
	super aquam refectionis educavit me,
	>>>>>>>>>> new9.txt
	animam meam convertit,
	deduxit me super semitas jusitiae,
	propter nomen suum.
	Nam et si ambulavero in medio umbrae mortis,
	non timebo mala, quoniam TU mecum es:
	virga tua et baculus tuus ipsa me consolata sunt.
	EOF

	test_must_fail git merge-file -p --marker-size=10 --diff3 \
		new8.txt new5.txt new9.txt >actual &&
	test_cmp expect actual
'

test_expect_success 'conflict at EOF without LF resolved by --ours' '
	printf "line1\nline2\nline3" >nolf-orig.txt &&
	printf "line1\nline2\nline3x" >nolf-diff1.txt &&
	printf "line1\nline2\nline3y" >nolf-diff2.txt &&

	git merge-file -p --ours nolf-diff1.txt nolf-orig.txt nolf-diff2.txt >output.txt &&
	printf "line1\nline2\nline3x" >expect.txt &&
	test_cmp expect.txt output.txt
'

test_expect_success 'conflict at EOF without LF resolved by --theirs' '
	printf "line1\nline2\nline3" >nolf-orig.txt &&
	printf "line1\nline2\nline3x" >nolf-diff1.txt &&
	printf "line1\nline2\nline3y" >nolf-diff2.txt &&
	git merge-file -p --theirs nolf-diff1.txt nolf-orig.txt nolf-diff2.txt >output.txt &&
	printf "line1\nline2\nline3y" >expect.txt &&
	test_cmp expect.txt output.txt
'

test_expect_success 'conflict at EOF without LF resolved by --union' '
	printf "line1\nline2\nline3" >nolf-orig.txt &&
	printf "line1\nline2\nline3x" >nolf-diff1.txt &&
	printf "line1\nline2\nline3y" >nolf-diff2.txt &&
	git merge-file -p --union nolf-diff1.txt nolf-orig.txt nolf-diff2.txt >output.txt &&
	printf "line1\nline2\nline3x\nline3y" >expect.txt &&
	test_cmp expect.txt output.txt
'

# ---- more merge-file tests ----

test_expect_success 'merge-file -p sends to stdout, does not modify input' '
	cp new1.txt input.txt &&
	md5before=$(md5sum input.txt | cut -d" " -f1) &&
	git merge-file -p input.txt orig.txt new2.txt >stdout.txt &&
	md5after=$(md5sum input.txt | cut -d" " -f1) &&
	test "$md5before" = "$md5after"
'

test_expect_success 'merge with all three labels via -L' '
	cp backup.txt test.txt &&
	test_must_fail git merge-file -L ours -L base -L theirs test.txt orig.txt new3.txt &&
	grep "<<<<<<< ours" test.txt &&
	grep ">>>>>>> theirs" test.txt
'

test_expect_success 'merge with only two -L labels' '
	cp backup.txt test.txt &&
	test_must_fail git merge-file -L mine -L ancestor test.txt orig.txt new3.txt &&
	grep "<<<<<<< mine" test.txt &&
	grep ">>>>>>> new3.txt" test.txt
'

test_expect_success 'merge-file --quiet suppresses conflict warnings' '
	cp backup.txt test.txt &&
	test_must_fail git merge-file --quiet test.txt orig.txt new3.txt 2>stderr &&
	test_must_be_empty stderr
'

test_expect_success 'merge with identical our and their produces clean result' '
	cp new1.txt ident1.txt &&
	cp new1.txt ident2.txt &&
	git merge-file -p ident1.txt orig.txt ident2.txt >out &&
	test_cmp new1.txt out
'

test_expect_success 'merge-file with --diff3 shows base marker' '
	printf "A\nB\nC\n" >d3base.txt &&
	printf "A\nX\nC\n" >d3ours.txt &&
	printf "A\nY\nC\n" >d3theirs.txt &&
	test_must_fail git merge-file --diff3 -p d3ours.txt d3base.txt d3theirs.txt >d3out &&
	grep "|||||||" d3out
'

test_expect_success 'merge-file exit code is number of conflicts' '
	cp backup.txt test.txt &&
	test_expect_code 1 git merge-file test.txt orig.txt new3.txt
'

test_expect_success 'merge-file clean merge returns 0' '
	cp new1.txt clean1.txt &&
	git merge-file clean1.txt orig.txt new2.txt &&
	test $? -eq 0
'

test_expect_success 'merge-file --zdiff3 is accepted' '
	printf "A\nB\nC\n" >zd3base.txt &&
	printf "A\nX\nC\n" >zd3ours.txt &&
	printf "A\nY\nC\n" >zd3theirs.txt &&
	test_must_fail git merge-file --zdiff3 -p zd3ours.txt zd3base.txt zd3theirs.txt >zd3out &&
	grep "<<<<<<< zd3ours.txt" zd3out
'

test_expect_success 'merge-file -p with no conflict does not modify file' '
	cp orig.txt nomod.txt &&
	git merge-file -p nomod.txt orig.txt orig.txt >out2 &&
	test_cmp orig.txt nomod.txt
'

test_done
