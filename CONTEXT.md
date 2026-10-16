# Coxswain

A two-panel file manager for the terminal and the desktop. This glossary names the concepts its code and docs share, so both apps and every contributor use the same words.

## Version control

**Version control system**:
A program that keeps the history of a folder tree, such as git, Jujutsu, Mercurial, Subversion or CVS.
_Avoid_: VCS (in user-facing text), SCM, source control

**Repository**:
A folder tree under a version control system, from its root down.
_Avoid_: repo (in user-facing text), project

**Working copy**:
The files of a repository as they are on disk, against which changes are shown.
_Avoid_: checkout, work tree (except for git worktrees)

**Upstream**:
The branch on a remote that a local branch follows, as the version control system records it.
_Avoid_: tracking branch, remote branch

**Against upstream**:
The view of what differs between a branch and its upstream: the commits only here (not pushed) and the commits only there (not merged), as of the last fetch.
_Avoid_: diff with remote, incoming/outgoing

**Handoff**:
Opening the user's own tool (a git client, or a terminal in the repository) for what Coxswain does not do itself.
_Avoid_: external tool, integration
