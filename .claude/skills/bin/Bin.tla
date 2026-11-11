---------------------------------- MODULE Bin ----------------------------------
(* The freedesktop.org bin as Coxswain's code works it, step by step, with two programs on the
   same bin (the terminal app and the desktop app), or Coxswain and another program that takes
   no lock of ours (the desktop's own trash: Outside), and a crash possible between any two
   steps. Every action below names the code it stands for: crates/coxswain-core/src/bin.rs (mod
   fd) and crates/coxswain-core/src/xdg_trash.rs. One step of the model is one system call of
   the code. Read it with .claude/skills/bin/SKILL.md.

   The world is small on purpose: NC files (the user's "report.txt", all with one name, so names
   collide in the bin and at the origin), NN names the bin can give them (report.txt,
   report.txt (2)), NS places they can be restored to (~/Documents/report.txt and, beside it,
   report.txt (2)), NA Coxswains, and with Outside one other program. 0 stands for nothing. *)

EXTENDS Naturals, FiniteSets, TLC

CONSTANTS NA, NC, NN, NS,
          Variant,        \* "code": the code as it is; a broken variant otherwise (check.sh)
          PutDuringEmpty, \* FALSE: assume no put overlaps an empty (the known finding set aside)
          Outside         \* TRUE: one more program, which takes no lock of ours

As == 1..NA
O == NA + 1
Ps == IF Outside THEN 1..NA+1 ELSE As
Cs == 1..NC
Ns == 1..NN
Ss == 1..NS
None == 0
Claimed == NS + 1  \* an info file made by create_new whose body is not written yet

(* Re-checks before each removal (#726): "no_recheck" is the code before them. A re-check and
   the call it guards are two system calls, so two steps; "atomic" makes them one, which the
   code cannot: it shows what is left is only the instant between the two. "link_open" is
   "atomic" but for the link, which has no re-check (RId skipped), so any file at files/<n>,
   another program's too, can be linked at the place (#901); "back_by_name" is "link_open" with that link taken
   away again by name, without looking at what is at the place by then (#923). *)
Checked == Variant # "no_recheck"
LinkOpen == Variant \in {"link_open", "back_by_name"}
Split == Variant \notin {"atomic", "link_open", "back_by_name"}
Guard(c) == Split \/ c

VARIABLES
  origin,  \* origin[s]: the file at place s of ~/Documents, or None
  files,   \* files[n]: the file at files/<n> in the bin, or None
  info,    \* info[n]: the place info/<n>.trashinfo's Path names, Claimed (empty file), or None
  iid,     \* iid[n]: which info file is at info/<n> (its dev, ino): the file whose put made it
  wanted,  \* the files the user has not deleted for good or emptied: they must still exist
  pc,      \* pc[a]: the step program a is between
  loc,     \* loc[a]: what program a holds in its variables
  lock     \* the program holding the bin's .coxswain-lock (fd::hold), or None

vars == <<origin, files, info, iid, wanted, pc, loc, lock>>

Nowhere(c) == (\A s \in Ss : origin[s] # c) /\ (\A n \in Ns : files[n] # c)
\* list_in: an info file whose Path parses, and the item it names, both there
Listed(n) == info[n] \in Ss /\ files[n] # None
\* fd::ids at the look, still so: the same file at files/<n>, the same info file at info/<n>
SameFile(a) == files[loc[a].n] = loc[a].c
SameInfo(a) == info[loc[a].n] # None /\ iid[loc[a].n] = loc[a].i
Idle(a) == pc[a] = "idle"
Fresh == [n |-> 0, s |-> 0, to |-> 0, c |-> 0, i |-> 0, free |-> FALSE, todo |-> {}, strays |-> {}]
Go(a, p, l) == pc' = [pc EXCEPT ![a] = p] /\ loc' = [loc EXCEPT ![a] = l] /\ UNCHANGED lock
\* The step ends the operation: the lock, when held, is let go (fd::hold's files dropped).
Done(a) == /\ pc' = [pc EXCEPT ![a] = "idle"] /\ loc' = [loc EXCEPT ![a] = Fresh]
           /\ lock' = IF lock = a THEN None ELSE lock
NoInfo(n) == info' = [info EXCEPT ![n] = None] /\ iid' = [iid EXCEPT ![n] = None]
(* restore_in, delete_in and empty_in begin with fd::hold: flock(2) on the bin's lock file,
   waiting while another program holds it. The "unlocked" variant is the code before #720. *)
Free == Variant = "unlocked" \/ lock = None
Take(a, p, l) == /\ pc' = [pc EXCEPT ![a] = p] /\ loc' = [loc EXCEPT ![a] = l]
                 /\ lock' = IF Variant = "unlocked" THEN lock ELSE a
Emptying(a) == pc[a] \in {"e_file", "e_check", "e_remove", "e_info_check", "e_info",
                          "e_sweep_check", "e_sweep_rm", "e_sweep_files", "e_sweep_info"}
Putting(a) == pc[a] \in {"put_check", "put_claim", "put_write", "put_move", "put_fail"}

Init ==
  /\ origin = [s \in Ss |-> IF s = 1 THEN 1 ELSE None]
  /\ files = [n \in Ns |-> None]
  /\ info = [n \in Ns |-> None]
  /\ iid = [n \in Ns |-> None]
  /\ wanted = {1}
  /\ pc = [a \in Ps |-> "idle"]
  /\ loc = [a \in Ps |-> Fresh]
  /\ lock = None

(* What a program holds of file c: as an item it looked at (fd::ids, a stray's id), or as the
   put of an info file still there. *)
Tuples(a) == pc[a] \in {"e_file", "e_check", "e_remove", "e_info_check", "e_info"}
Held(a) == {loc[a].c, loc[a].i} \cup {p[2] : p \in loc[a].strays}
           \cup (IF Tuples(a) THEN {p[2] : p \in loc[a].todo} \cup {p[3] : p \in loc[a].todo} ELSE {})
           \cup (IF pc[a] \in {"e_sweep_check", "e_sweep_rm"} THEN {p[2] : p \in loc[a].todo} ELSE {})
(* A file deleted for good is reused as a new one, so the world stays finite; but not while a
   program still holds it, as fd::id (device, inode, change time) tells a new file from it. *)
Reusable(c) == c \notin wanted /\ Nowhere(c) /\ \A a \in Ps : c \notin Held(a)
               /\ \A n \in Ns : iid[n] # c

(* The user (or any program) saves a new file at a free place: what makes a restore find its
   place taken. *)
Make(s) ==
  /\ origin[s] = None
  /\ \E c \in Cs : Reusable(c)
  /\ LET c == CHOOSE c \in Cs : Reusable(c) IN
       /\ origin' = [origin EXCEPT ![s] = c]
       /\ wanted' = wanted \cup {c}
  /\ UNCHANGED <<files, info, iid, pc, loc, lock>>

(* Put: F8 on a file, or any program's put. xdg_trash::put_in (Termux, illumos); the trash
   crate's move_to_trash (everywhere else) takes the same steps in the same order, with a
   placeholder at files/<n> that makes its rename refuse to replace. For each name in
   name_candidates order: symlink_metadata(files/<n>) must fail (put_check), then
   create_new(info/<n>.trashinfo) claims the name or, AlreadyExists, the next name is tried
   (put_claim); write_all of the body (put_write); rename(path, files/<n>) (put_move); on a
   failed rename remove_file(info) (put_fail). *)
PutStart(a, s) ==
  /\ Idle(a) /\ origin[s] # None
  /\ PutDuringEmpty \/ \A b \in Ps : ~Emptying(b)
  /\ Go(a, "put_check", [Fresh EXCEPT !.s = s, !.n = 1, !.c = origin[s]])
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

NextName(a) ==
  IF loc[a].n < NN THEN Go(a, "put_check", [loc[a] EXCEPT !.n = @ + 1]) ELSE Done(a)

PutCheck(a) ==
  /\ pc[a] = "put_check"
  /\ IF files[loc[a].n] = None THEN Go(a, "put_claim", loc[a]) ELSE NextName(a)
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

\* A new info file: a new (dev, ino), told by the file being put.
PutClaim(a) ==
  /\ pc[a] = "put_claim"
  /\ IF info[loc[a].n] = None
       THEN /\ info' = [info EXCEPT ![loc[a].n] = Claimed]
            /\ iid' = [iid EXCEPT ![loc[a].n] = loc[a].c]
            /\ Go(a, IF Variant = "put_moves_first" THEN "put_move" ELSE "put_write", loc[a])
       ELSE NextName(a) /\ UNCHANGED <<info, iid>>
  /\ UNCHANGED <<origin, files, wanted>>

PutWrite(a) ==
  /\ pc[a] = "put_write"
  /\ info' = [info EXCEPT ![loc[a].n] = loc[a].s]
  /\ IF Variant = "put_moves_first" THEN Done(a) ELSE Go(a, "put_move", loc[a])
  /\ UNCHANGED <<origin, files, iid, wanted>>

\* rename(2) replaces whatever is at files/<n> by then: put_check guarded against that earlier.
PutMove(a) ==
  /\ pc[a] = "put_move"
  /\ IF origin[loc[a].s] = None
       THEN Go(a, "put_fail", loc[a]) /\ UNCHANGED <<origin, files>>
       ELSE /\ files' = [files EXCEPT ![loc[a].n] = origin[loc[a].s]]
            /\ origin' = [origin EXCEPT ![loc[a].s] = None]
            /\ IF Variant = "put_moves_first" THEN Go(a, "put_write", loc[a]) ELSE Done(a)
  /\ UNCHANGED <<info, iid, wanted>>

PutFail(a) ==
  /\ pc[a] = "put_fail"
  /\ NoInfo(loc[a].n)
  /\ Done(a)
  /\ UNCHANGED <<origin, files, wanted>>

(* Restore: bin::restore -> fd::restore_in, for a file (a folder goes by rename and is not
   modelled). find(bins, p) lists the item and fd::ids its file and info file (r_start);
   symlink_metadata(origin) says whether its place is taken: left in the bin as `taken`, or with
   free_name the first free name beside it (r_check); the file still the one found (r_id);
   move_new: hard_link(files/<n>, to), which fails when `to` exists by then or the item is gone
   (r_link); the file now at `to` the one found (its dev, ino: the link changes its change
   time), else another program's put since, which stays in the bin (#901), and its link at `to`
   is taken away again while `to` and files/<n> are still one file (dev, ino, change time), so
   nothing leaves the disk (#923) (r_back_check, r_back); files/<n> still the file now at `to` (r_unlink_check); remove_file(files/<n>)
   (r_unlink); either gone or another's by then: move_new says false and the info file stays,
   the other program's to remove; the info file still the one found (r_info_check);
   remove_file(info) (r_info). *)
RestoreStart(a, n, free) ==
  /\ a \in As /\ Idle(a) /\ Free /\ Listed(n)
  /\ Take(a, "r_check", [Fresh EXCEPT !.n = n, !.s = info[n], !.free = free, !.c = files[n], !.i = iid[n]])
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

FirstFree == CHOOSE t \in Ss : origin[t] = None /\ \A u \in Ss : origin[u] = None => t <= u
RAfterCheck == IF Checked /\ Split THEN "r_id" ELSE "r_link"

RCheck(a) ==
  /\ pc[a] = "r_check"
  /\ IF origin[loc[a].s] = None THEN Go(a, RAfterCheck, [loc[a] EXCEPT !.to = loc[a].s])
     ELSE IF ~loc[a].free THEN Done(a)                        \* taken: asked about later
     ELSE IF \E t \in Ss : origin[t] = None THEN Go(a, RAfterCheck, [loc[a] EXCEPT !.to = FirstFree])
     ELSE Done(a)                                             \* no free name within NS
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

RId(a) ==
  /\ pc[a] = "r_id"
  /\ IF SameFile(a) THEN Go(a, "r_link", loc[a]) ELSE Done(a) \* gone or another: not restored
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

RLink(a) ==
  /\ pc[a] = "r_link"
  /\ LET n == loc[a].n  to == loc[a].to IN
     IF Variant = "restore_renames"
       THEN \* a plain rename(files/<n>, to): replaces what is at `to` by now
            IF files[n] = None THEN Done(a) /\ UNCHANGED <<origin, files>>
            ELSE /\ origin' = [origin EXCEPT ![to] = files[n]]
                 /\ files' = [files EXCEPT ![n] = None]
                 /\ Go(a, "r_info", loc[a])
       ELSE IF files[n] = None \/ origin[to] # None \/ ~(LinkOpen \/ Guard(SameFile(a)))
            THEN Done(a) /\ UNCHANGED <<origin, files>>    \* hard_link fails: listed in `failed`
            ELSE /\ origin' = [origin EXCEPT ![to] = files[n]]
                 /\ Go(a, IF Checked /\ (Split \/ LinkOpen) THEN "r_unlink_check" ELSE "r_unlink", loc[a])
                 /\ UNCHANGED files
  /\ UNCHANGED <<info, iid, wanted>>

\* fd::id(to) against the id found (#901), then fd::id(from) == fd::id(to): files/<n> is still
\* the file just linked at `to`.
RUnlinkCheck(a) ==
  /\ pc[a] = "r_unlink_check"
  /\ IF origin[loc[a].to] = None THEN Done(a)     \* gone from `to` already: left in the bin
     ELSE IF origin[loc[a].to] # loc[a].c        \* another's: left in the bin, its link taken back
       THEN Go(a, IF Split THEN "r_back_check" ELSE "r_back", loc[a])
     ELSE IF files[loc[a].n] = origin[loc[a].to] THEN Go(a, "r_unlink", loc[a])
     ELSE Done(a)                         \* another's now, or gone: left, and its info file
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

\* fd::id(to) == fd::id(from): `to` still a link of the file in the bin, else a file saved there
\* since: left.
Linked(a) == origin[loc[a].to] # None /\ origin[loc[a].to] = files[loc[a].n]
RBackCheck(a) ==
  /\ pc[a] = "r_back_check"
  /\ IF Linked(a) THEN Go(a, "r_back", loc[a]) ELSE Done(a)
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

\* remove_file(to): the error said, the restore reports err.not_in_trash.
RBack(a) ==
  /\ pc[a] = "r_back"
  /\ IF (Variant = "back_by_name" \/ Guard(Linked(a))) /\ origin[loc[a].to] # None
       THEN origin' = [origin EXCEPT ![loc[a].to] = None]
       ELSE UNCHANGED origin
  /\ Done(a)
  /\ UNCHANGED <<files, info, iid, wanted>>

RAfterUnlink == IF Checked /\ Split THEN "r_info_check" ELSE "r_info"

RUnlink(a) ==
  /\ pc[a] = "r_unlink"
  /\ IF files[loc[a].n] = None \/ ~Guard(files[loc[a].n] = origin[loc[a].to])
       THEN Done(a) /\ UNCHANGED files    \* gone (moved all the same) or another's: info stays
     ELSE files' = [files EXCEPT ![loc[a].n] = None] /\ Go(a, RAfterUnlink, loc[a])
  /\ UNCHANGED <<origin, info, iid, wanted>>

RInfoCheck(a) ==
  /\ pc[a] = "r_info_check"
  /\ IF SameInfo(a) THEN Go(a, "r_info", loc[a]) ELSE Done(a)
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

RInfo(a) ==
  /\ pc[a] = "r_info"
  /\ IF Guard(SameInfo(a)) THEN NoInfo(loc[a].n) ELSE UNCHANGED <<info, iid>>
  /\ Done(a)
  /\ UNCHANGED <<origin, files, wanted>>

(* Delete for good: bin::delete -> fd::delete_in. find(bins, p) lists the item, which the user
   confirmed by name and size (d_start: from then on it need not exist), and fd::ids its file and
   info file; the file still the one found, else it is done: gone, or another's (d_check);
   remove(p), gone already counting as done (d_remove); the info file still the one found
   (d_info_check); remove_file(info) (d_info). *)
DeleteStart(a, n) ==
  /\ a \in As /\ Idle(a) /\ Free /\ Listed(n)
  /\ wanted' = wanted \ {files[n]}
  /\ Take(a, IF Checked /\ Split THEN "d_check" ELSE "d_remove",
          [Fresh EXCEPT !.n = n, !.c = files[n], !.i = iid[n]])
  /\ UNCHANGED <<origin, files, info, iid>>

DCheck(a) ==
  /\ pc[a] = "d_check"
  /\ IF SameFile(a) THEN Go(a, "d_remove", loc[a]) ELSE Done(a)
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

DRemove(a) ==
  /\ pc[a] = "d_remove"
  /\ IF files[loc[a].n] = None \/ ~Guard(SameFile(a))
       THEN Done(a) /\ UNCHANGED files                        \* gone: info stays
       ELSE files' = [files EXCEPT ![loc[a].n] = None]
            /\ Go(a, IF Checked /\ Split THEN "d_info_check" ELSE "d_info", loc[a])
  /\ UNCHANGED <<origin, info, iid, wanted>>

DInfoCheck(a) ==
  /\ pc[a] = "d_info_check"
  /\ IF SameInfo(a) THEN Go(a, "d_info", loc[a]) ELSE Done(a)
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

DInfo(a) ==
  /\ pc[a] = "d_info"
  /\ IF Guard(SameInfo(a)) THEN NoInfo(loc[a].n) ELSE UNCHANGED <<info, iid>>
  /\ Done(a)
  /\ UNCHANGED <<origin, files, wanted>>

(* Empty: bin::empty -> fd::empty_in. list_in lists what is in the bin, which the user confirmed
   by count and size, with fd::ids of each item's file and info file, and read_dir(files) keeps
   each entry whose info file is missing or empty (a put that died before writing it: Claimed),
   a stray left by a crash, with its (dev, ino) (e_start); delete_listed on the list, one item
   at a time (e_file): the same checks and calls as delete for good (e_check, e_remove,
   e_info_check, e_info); then for each stray its info must still be missing or empty and its
   (dev, ino) the same (e_sweep_check) before remove (e_sweep_rm). Info files without their item
   stay. The "sweep_all" variant is the code before #720: read_dir(files) and remove each entry
   (e_sweep_files), read_dir(info) and remove_file each (e_sweep_info). *)
EmptyStart(a) ==
  /\ a \in As /\ Idle(a) /\ Free
  /\ PutDuringEmpty \/ \A b \in Ps : ~Putting(b)
  /\ LET S == {n \in Ns : Listed(n)} IN
       /\ wanted' = wanted \ {files[n] : n \in S}
       /\ Take(a, "e_file", [Fresh EXCEPT !.todo = {<<n, files[n], iid[n]>> : n \in S},
                                          !.strays = {<<n, files[n]>> : n \in {m \in Ns : files[m] # None /\ info[m] \notin Ss}}])
  /\ UNCHANGED <<origin, files, info, iid>>

EFile(a) ==
  /\ pc[a] = "e_file"
  /\ IF loc[a].todo = {}
       THEN IF Variant = "sweep_all"
              THEN Go(a, "e_sweep_files", [Fresh EXCEPT !.todo = {n \in Ns : files[n] # None}])
              ELSE Go(a, "e_sweep_check", [Fresh EXCEPT !.todo = loc[a].strays])
       ELSE \E p \in loc[a].todo :
            Go(a, IF Checked /\ Split THEN "e_check" ELSE "e_remove",
               [loc[a] EXCEPT !.todo = @ \ {p}, !.n = p[1], !.c = p[2], !.i = p[3]])
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

ECheck(a) ==
  /\ pc[a] = "e_check"
  /\ Go(a, IF SameFile(a) THEN "e_remove" ELSE "e_file", loc[a])
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

ERemove(a) ==
  /\ pc[a] = "e_remove"
  /\ IF files[loc[a].n] = None \/ ~Guard(SameFile(a))
       THEN Go(a, "e_file", loc[a]) /\ UNCHANGED files
       ELSE files' = [files EXCEPT ![loc[a].n] = None]
            /\ Go(a, IF Checked /\ Split THEN "e_info_check" ELSE "e_info", loc[a])
  /\ UNCHANGED <<origin, info, iid, wanted>>

EInfoCheck(a) ==
  /\ pc[a] = "e_info_check"
  /\ Go(a, IF SameInfo(a) THEN "e_info" ELSE "e_file", loc[a])
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

EInfo(a) ==
  /\ pc[a] = "e_info"
  /\ IF Guard(SameInfo(a)) THEN NoInfo(loc[a].n) ELSE UNCHANGED <<info, iid>>
  /\ Go(a, "e_file", loc[a])
  /\ UNCHANGED <<origin, files, wanted>>

ESweepCheck(a) ==
  /\ pc[a] = "e_sweep_check"
  /\ IF loc[a].todo = {} THEN Done(a)
     ELSE \E p \in loc[a].todo :
          IF info[p[1]] \notin Ss /\ files[p[1]] = p[2]
            THEN Go(a, "e_sweep_rm", [loc[a] EXCEPT !.n = p[1], !.todo = @ \ {p}])
            ELSE Go(a, "e_sweep_check", [loc[a] EXCEPT !.todo = @ \ {p}])
  /\ UNCHANGED <<origin, files, info, iid, wanted>>

ESweepRm(a) ==
  /\ pc[a] = "e_sweep_rm"
  /\ files' = [files EXCEPT ![loc[a].n] = None]
  /\ Go(a, "e_sweep_check", loc[a])
  /\ UNCHANGED <<origin, info, iid, wanted>>

ESweepFiles(a) ==
  /\ pc[a] = "e_sweep_files"
  /\ IF loc[a].todo = {}
       THEN Go(a, "e_sweep_info", [Fresh EXCEPT !.todo = {n \in Ns : info[n] # None}])
            /\ UNCHANGED files
       ELSE \E n \in loc[a].todo :
            files' = [files EXCEPT ![n] = None] /\ Go(a, "e_sweep_files", [loc[a] EXCEPT !.todo = @ \ {n}])
  /\ UNCHANGED <<origin, info, iid, wanted>>

ESweepInfo(a) ==
  /\ pc[a] = "e_sweep_info"
  /\ IF loc[a].todo = {}
       THEN Done(a) /\ UNCHANGED <<info, iid>>
       ELSE \E n \in loc[a].todo :
            NoInfo(n) /\ Go(a, "e_sweep_info", [loc[a] EXCEPT !.todo = @ \ {n}])
  /\ UNCHANGED <<origin, files, wanted>>

(* The other program (Outside): the desktop's own trash, a browser, gio, trash-cli. It takes no
   lock of ours. Its own steps are taken to be right for itself: it removes or moves out an item
   it lists in one step, the file it listed (o_delete, o_restore, never over a file at the
   place), then that item's info file (o_info). Its Empty is o_delete of each item. Its puts are
   PutStart above. What it does to itself is its own; what it may do to Coxswain's work is the
   question. *)
ODelete(n) ==
  /\ Outside /\ Idle(O) /\ Listed(n)
  /\ wanted' = wanted \ {files[n]}
  /\ files' = [files EXCEPT ![n] = None]
  /\ Go(O, "o_info", [Fresh EXCEPT !.n = n])
  /\ UNCHANGED <<origin, info, iid>>

ORestore(n) ==
  /\ Outside /\ Idle(O) /\ Listed(n) /\ origin[info[n]] = None
  /\ origin' = [origin EXCEPT ![info[n]] = files[n]]
  /\ files' = [files EXCEPT ![n] = None]
  /\ Go(O, "o_info", [Fresh EXCEPT !.n = n])
  /\ UNCHANGED <<info, iid, wanted>>

OInfo ==
  /\ Outside /\ pc[O] = "o_info"
  /\ NoInfo(loc[O].n)
  /\ Done(O)
  /\ UNCHANGED <<origin, files, wanted>>

\* The program dies (or the machine) between two steps: what it held is gone, the disk stays.
\* flock(2) is let go with the process.
Crash(a) == ~Idle(a) /\ Done(a) /\ UNCHANGED <<origin, files, info, iid, wanted>>

Next ==
  \/ \E s \in Ss : Make(s)
  \/ \E n \in Ns : ODelete(n) \/ ORestore(n)
  \/ OInfo
  \/ \E a \in Ps :
       \/ \E s \in Ss : PutStart(a, s)
       \/ PutCheck(a) \/ PutClaim(a) \/ PutWrite(a) \/ PutMove(a) \/ PutFail(a)
       \/ \E n \in Ns, free \in BOOLEAN : RestoreStart(a, n, free)
       \/ RCheck(a) \/ RId(a) \/ RLink(a) \/ RUnlinkCheck(a) \/ RBackCheck(a) \/ RBack(a) \/ RUnlink(a) \/ RInfoCheck(a) \/ RInfo(a)
       \/ \E n \in Ns : DeleteStart(a, n)
       \/ DCheck(a) \/ DRemove(a) \/ DInfoCheck(a) \/ DInfo(a)
       \/ EmptyStart(a) \/ EFile(a) \/ ECheck(a) \/ ERemove(a) \/ EInfoCheck(a) \/ EInfo(a)
       \/ ESweepCheck(a) \/ ESweepRm(a) \/ ESweepFiles(a) \/ ESweepInfo(a)
       \/ Crash(a)

Spec == Init /\ [][Next]_vars

--------------------------------------------------------------------------------
(* The promises. Each in one sentence in SKILL.md. *)

\* A file the user put in the bin, and has not deleted for good or emptied, still exists:
\* at its place, beside it, or in the bin.
NoLoss == \A c \in wanted : ~Nowhere(c)

\* Such a file, when it is only in the bin, is listed there (its info file names its place),
\* so the user can see it and restore it.
Restorable == \A c \in wanted :
  (\E s \in Ss : origin[s] = c) \/ (\E n \in Ns : files[n] = c /\ info[n] \in Ss)

\* A file at its place is never replaced by another: it only ever leaves by being put in the bin.
NeverOverwrite == [][\A s \in Ss : origin[s] # None /\ origin'[s] # origin[s] => origin'[s] = None]_vars

TypeOK ==
  /\ origin \in [Ss -> Cs \cup {None}]
  /\ files \in [Ns -> Cs \cup {None}]
  /\ info \in [Ns -> Ss \cup {None, Claimed}]
  /\ iid \in [Ns -> Cs \cup {None}]
  /\ wanted \subseteq Cs
  /\ lock \in As \cup {None}
================================================================================
