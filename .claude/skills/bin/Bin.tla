---------------------------------- MODULE Bin ----------------------------------
(* The freedesktop.org bin as Coxswain's code works it, step by step, with two programs on the
   same bin (the terminal app and the desktop app, or Coxswain and the desktop's own trash)
   and a crash possible between any two steps. Every action below names the code it stands for:
   crates/coxswain-core/src/bin.rs (mod fd) and crates/coxswain-core/src/xdg_trash.rs. One step
   of the model is one system call of the code. Read it with .claude/skills/bin/SKILL.md.

   The world is small on purpose: NC files (the user's "report.txt", all with one name, so names
   collide in the bin and at the origin), NN names the bin can give them (report.txt,
   report.txt (2)), NS places they can be restored to (~/Documents/report.txt and, beside it,
   report.txt (2)), NA programs. 0 stands for nothing. *)

EXTENDS Naturals, FiniteSets, TLC

CONSTANTS NA, NC, NN, NS,
          Variant,        \* "code": the code as it is; a broken variant otherwise (check.sh)
          PutDuringEmpty  \* FALSE: assume no put overlaps an empty (the known finding set aside)

As == 1..NA
Cs == 1..NC
Ns == 1..NN
Ss == 1..NS
None == 0
Claimed == NS + 1  \* an info file made by create_new whose body is not written yet

VARIABLES
  origin,  \* origin[s]: the file at place s of ~/Documents, or None
  files,   \* files[n]: the file at files/<n> in the bin, or None
  info,    \* info[n]: the place info/<n>.trashinfo's Path names, Claimed (empty file), or None
  wanted,  \* the files the user has not deleted for good or emptied: they must still exist
  pc,      \* pc[a]: the step program a is between
  loc,     \* loc[a]: what program a holds in its variables
  lock     \* the program holding the bin's .coxswain-lock (fd::hold), or None

vars == <<origin, files, info, wanted, pc, loc, lock>>

Nowhere(c) == (\A s \in Ss : origin[s] # c) /\ (\A n \in Ns : files[n] # c)
\* list_in: an info file whose Path parses, and the item it names, both there
Listed(n) == info[n] \in Ss /\ files[n] # None
Idle(a) == pc[a] = "idle"
Fresh == [n |-> 0, s |-> 0, to |-> 0, free |-> FALSE, todo |-> {}, strays |-> {}]
Go(a, p, l) == pc' = [pc EXCEPT ![a] = p] /\ loc' = [loc EXCEPT ![a] = l] /\ UNCHANGED lock
\* The step ends the operation: the lock, when held, is let go (fd::hold's files dropped).
Done(a) == /\ pc' = [pc EXCEPT ![a] = "idle"] /\ loc' = [loc EXCEPT ![a] = Fresh]
           /\ lock' = IF lock = a THEN None ELSE lock
(* restore_in, delete_in and empty_in begin with fd::hold: flock(2) on the bin's lock file,
   waiting while another program holds it. The "unlocked" variant is the code before #720. *)
Free == Variant = "unlocked" \/ lock = None
Take(a, p, l) == /\ pc' = [pc EXCEPT ![a] = p] /\ loc' = [loc EXCEPT ![a] = l]
                 /\ lock' = IF Variant = "unlocked" THEN lock ELSE a
Emptying(a) == pc[a] \in {"e_file", "e_info", "e_sweep_check", "e_sweep_rm", "e_sweep_files", "e_sweep_info"}
Putting(a) == pc[a] \in {"put_check", "put_claim", "put_write", "put_move", "put_fail"}

Init ==
  /\ origin = [s \in Ss |-> IF s = 1 THEN 1 ELSE None]
  /\ files = [n \in Ns |-> None]
  /\ info = [n \in Ns |-> None]
  /\ wanted = {1}
  /\ pc = [a \in As |-> "idle"]
  /\ loc = [a \in As |-> Fresh]
  /\ lock = None

(* The user (or any program) saves a new file at a free place: what makes a restore find its
   place taken. A file deleted for good is reused as the new one, so the world stays finite. *)
Make(s) ==
  /\ origin[s] = None
  /\ \E c \in Cs : c \notin wanted /\ Nowhere(c)
  /\ LET c == CHOOSE c \in Cs : c \notin wanted /\ Nowhere(c) IN
       /\ origin' = [origin EXCEPT ![s] = c]
       /\ wanted' = wanted \cup {c}
  /\ UNCHANGED <<files, info, pc, loc, lock>>

(* Put: F8 on a file. xdg_trash::put_in (Termux, illumos); the trash crate's move_to_trash
   (everywhere else) takes the same steps in the same order, with a placeholder at files/<n>
   that makes its rename refuse to replace. For each name in name_candidates order:
   symlink_metadata(files/<n>) must fail (put_check), then create_new(info/<n>.trashinfo)
   claims the name or, AlreadyExists, the next name is tried (put_claim); write_all of the body
   (put_write); rename(path, files/<n>) (put_move); on a failed rename remove_file(info) (put_fail). *)
PutStart(a, s) ==
  /\ Idle(a) /\ origin[s] # None
  /\ PutDuringEmpty \/ \A b \in As : ~Emptying(b)
  /\ Go(a, "put_check", [Fresh EXCEPT !.s = s, !.n = 1])
  /\ UNCHANGED <<origin, files, info, wanted>>

NextName(a) ==
  IF loc[a].n < NN THEN Go(a, "put_check", [loc[a] EXCEPT !.n = @ + 1]) ELSE Done(a)

PutCheck(a) ==
  /\ pc[a] = "put_check"
  /\ IF files[loc[a].n] = None THEN Go(a, "put_claim", loc[a]) ELSE NextName(a)
  /\ UNCHANGED <<origin, files, info, wanted>>

PutClaim(a) ==
  /\ pc[a] = "put_claim"
  /\ IF info[loc[a].n] = None
       THEN /\ info' = [info EXCEPT ![loc[a].n] = Claimed]
            /\ Go(a, IF Variant = "put_moves_first" THEN "put_move" ELSE "put_write", loc[a])
       ELSE NextName(a) /\ UNCHANGED info
  /\ UNCHANGED <<origin, files, wanted>>

PutWrite(a) ==
  /\ pc[a] = "put_write"
  /\ info' = [info EXCEPT ![loc[a].n] = loc[a].s]
  /\ IF Variant = "put_moves_first" THEN Done(a) ELSE Go(a, "put_move", loc[a])
  /\ UNCHANGED <<origin, files, wanted>>

\* rename(2) replaces whatever is at files/<n> by then: put_check guarded against that earlier.
PutMove(a) ==
  /\ pc[a] = "put_move"
  /\ IF origin[loc[a].s] = None
       THEN Go(a, "put_fail", loc[a]) /\ UNCHANGED <<origin, files>>
       ELSE /\ files' = [files EXCEPT ![loc[a].n] = origin[loc[a].s]]
            /\ origin' = [origin EXCEPT ![loc[a].s] = None]
            /\ IF Variant = "put_moves_first" THEN Go(a, "put_write", loc[a]) ELSE Done(a)
  /\ UNCHANGED <<info, wanted>>

PutFail(a) ==
  /\ pc[a] = "put_fail"
  /\ info' = [info EXCEPT ![loc[a].n] = None]
  /\ Done(a)
  /\ UNCHANGED <<origin, files, wanted>>

(* Restore: bin::restore -> fd::restore_in, for a file (a folder goes by rename and is not
   modelled). find(bins, p) lists the item (r_start); symlink_metadata(origin) says whether its
   place is taken: left in the bin as `taken`, or with free_name the first free name beside it
   (r_check); move_new: hard_link(files/<n>, to), which fails when `to` exists by then or the
   item is gone (r_link); remove_file(files/<n>) (r_unlink); remove_file(info) (r_info). *)
RestoreStart(a, n, free) ==
  /\ Idle(a) /\ Free /\ Listed(n)
  /\ Take(a, "r_check", [Fresh EXCEPT !.n = n, !.s = info[n], !.free = free])
  /\ UNCHANGED <<origin, files, info, wanted>>

FirstFree == CHOOSE t \in Ss : origin[t] = None /\ \A u \in Ss : origin[u] = None => t <= u

RCheck(a) ==
  /\ pc[a] = "r_check"
  /\ IF origin[loc[a].s] = None THEN Go(a, "r_link", [loc[a] EXCEPT !.to = loc[a].s])
     ELSE IF ~loc[a].free THEN Done(a)                        \* taken: asked about later
     ELSE IF \E t \in Ss : origin[t] = None THEN Go(a, "r_link", [loc[a] EXCEPT !.to = FirstFree])
     ELSE Done(a)                                             \* no free name within NS
  /\ UNCHANGED <<origin, files, info, wanted>>

RLink(a) ==
  /\ pc[a] = "r_link"
  /\ LET n == loc[a].n  to == loc[a].to IN
     IF Variant = "restore_renames"
       THEN \* a plain rename(files/<n>, to): replaces what is at `to` by now
            IF files[n] = None THEN Done(a) /\ UNCHANGED <<origin, files>>
            ELSE /\ origin' = [origin EXCEPT ![to] = files[n]]
                 /\ files' = [files EXCEPT ![n] = None]
                 /\ Go(a, "r_info", loc[a])
       ELSE IF files[n] = None \/ origin[to] # None
            THEN Done(a) /\ UNCHANGED <<origin, files>>    \* hard_link fails: listed in `failed`
            ELSE /\ origin' = [origin EXCEPT ![to] = files[n]]
                 /\ Go(a, "r_unlink", loc[a])
                 /\ UNCHANGED files
  /\ UNCHANGED <<info, wanted>>

RUnlink(a) ==
  /\ pc[a] = "r_unlink"
  /\ IF files[loc[a].n] = None
       THEN Done(a) /\ UNCHANGED files                        \* remove_file fails: `failed`, info stays
       ELSE files' = [files EXCEPT ![loc[a].n] = None] /\ Go(a, "r_info", loc[a])
  /\ UNCHANGED <<origin, info, wanted>>

RInfo(a) ==
  /\ pc[a] = "r_info"
  /\ info' = [info EXCEPT ![loc[a].n] = None]
  /\ Done(a)
  /\ UNCHANGED <<origin, files, wanted>>

(* Delete for good: bin::delete -> fd::delete_in. find(bins, p) lists the item, which the user
   confirmed by name and size (d_start: from then on it need not exist); remove(p) (d_remove);
   remove_file(info) (d_info). *)
DeleteStart(a, n) ==
  /\ Idle(a) /\ Free /\ Listed(n)
  /\ wanted' = wanted \ {files[n]}
  /\ Take(a, "d_remove", [Fresh EXCEPT !.n = n])
  /\ UNCHANGED <<origin, files, info>>

DRemove(a) ==
  /\ pc[a] = "d_remove"
  /\ IF files[loc[a].n] = None
       THEN Done(a) /\ UNCHANGED files                        \* remove fails: `failed`, info stays
       ELSE files' = [files EXCEPT ![loc[a].n] = None] /\ Go(a, "d_info", loc[a])
  /\ UNCHANGED <<origin, info, wanted>>

DInfo(a) ==
  /\ pc[a] = "d_info"
  /\ info' = [info EXCEPT ![loc[a].n] = None]
  /\ Done(a)
  /\ UNCHANGED <<origin, files, wanted>>

(* Empty: bin::empty -> fd::empty_in. list_in lists what is in the bin, which the user confirmed
   by count and size, and read_dir(files) keeps each entry without an info file then, a stray
   left by a crash (e_start); delete_in on the list, item then info (e_file, e_info); then for
   each stray symlink_metadata(info) must still fail (e_sweep_check) before remove (e_sweep_rm).
   Info files without their item stay. The "sweep_all" variant is the code before #720:
   read_dir(files) and remove each entry (e_sweep_files), read_dir(info) and remove_file each
   (e_sweep_info). *)
EmptyStart(a) ==
  /\ Idle(a) /\ Free
  /\ PutDuringEmpty \/ \A b \in As : ~Putting(b)
  /\ LET S == {n \in Ns : Listed(n)} IN
       /\ wanted' = wanted \ {files[n] : n \in S}
       /\ Take(a, "e_file", [Fresh EXCEPT !.todo = S,
                                          !.strays = {n \in Ns : files[n] # None /\ info[n] = None}])
  /\ UNCHANGED <<origin, files, info>>

EFile(a) ==
  /\ pc[a] = "e_file"
  /\ IF loc[a].todo = {}
       THEN /\ IF Variant = "sweep_all"
                 THEN Go(a, "e_sweep_files", [Fresh EXCEPT !.todo = {n \in Ns : files[n] # None}])
                 ELSE Go(a, "e_sweep_check", [Fresh EXCEPT !.todo = loc[a].strays])
            /\ UNCHANGED files
       ELSE \E n \in loc[a].todo :
            IF files[n] = None
              THEN Go(a, "e_file", [loc[a] EXCEPT !.todo = @ \ {n}]) /\ UNCHANGED files
              ELSE files' = [files EXCEPT ![n] = None] /\ Go(a, "e_info", [loc[a] EXCEPT !.n = n])
  /\ UNCHANGED <<origin, info, wanted>>

EInfo(a) ==
  /\ pc[a] = "e_info"
  /\ info' = [info EXCEPT ![loc[a].n] = None]
  /\ Go(a, "e_file", [loc[a] EXCEPT !.todo = @ \ {loc[a].n}])
  /\ UNCHANGED <<origin, files, wanted>>

ESweepCheck(a) ==
  /\ pc[a] = "e_sweep_check"
  /\ IF loc[a].todo = {} THEN Done(a)
     ELSE \E n \in loc[a].todo :
          IF info[n] = None THEN Go(a, "e_sweep_rm", [loc[a] EXCEPT !.n = n])
          ELSE Go(a, "e_sweep_check", [loc[a] EXCEPT !.todo = @ \ {n}])
  /\ UNCHANGED <<origin, files, info, wanted>>

ESweepRm(a) ==
  /\ pc[a] = "e_sweep_rm"
  /\ files' = [files EXCEPT ![loc[a].n] = None]
  /\ Go(a, "e_sweep_check", [loc[a] EXCEPT !.todo = @ \ {loc[a].n}])
  /\ UNCHANGED <<origin, info, wanted>>

ESweepFiles(a) ==
  /\ pc[a] = "e_sweep_files"
  /\ IF loc[a].todo = {}
       THEN Go(a, "e_sweep_info", [Fresh EXCEPT !.todo = {n \in Ns : info[n] # None}])
            /\ UNCHANGED files
       ELSE \E n \in loc[a].todo :
            files' = [files EXCEPT ![n] = None] /\ Go(a, "e_sweep_files", [loc[a] EXCEPT !.todo = @ \ {n}])
  /\ UNCHANGED <<origin, info, wanted>>

ESweepInfo(a) ==
  /\ pc[a] = "e_sweep_info"
  /\ IF loc[a].todo = {}
       THEN Done(a) /\ UNCHANGED info
       ELSE \E n \in loc[a].todo :
            info' = [info EXCEPT ![n] = None] /\ Go(a, "e_sweep_info", [loc[a] EXCEPT !.todo = @ \ {n}])
  /\ UNCHANGED <<origin, files, wanted>>

\* The program dies (or the machine) between two steps: what it held is gone, the disk stays.
\* flock(2) is let go with the process.
Crash(a) == ~Idle(a) /\ Done(a) /\ UNCHANGED <<origin, files, info, wanted>>

Next ==
  \/ \E s \in Ss : Make(s)
  \/ \E a \in As :
       \/ \E s \in Ss : PutStart(a, s)
       \/ PutCheck(a) \/ PutClaim(a) \/ PutWrite(a) \/ PutMove(a) \/ PutFail(a)
       \/ \E n \in Ns, free \in BOOLEAN : RestoreStart(a, n, free)
       \/ RCheck(a) \/ RLink(a) \/ RUnlink(a) \/ RInfo(a)
       \/ \E n \in Ns : DeleteStart(a, n)
       \/ DRemove(a) \/ DInfo(a)
       \/ EmptyStart(a) \/ EFile(a) \/ EInfo(a) \/ ESweepCheck(a) \/ ESweepRm(a)
       \/ ESweepFiles(a) \/ ESweepInfo(a)
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
  /\ wanted \subseteq Cs
  /\ lock \in As \cup {None}
================================================================================
