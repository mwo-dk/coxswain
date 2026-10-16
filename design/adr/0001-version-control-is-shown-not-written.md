# Version control is shown, not written

Coxswain shows version control state (status, branches, history and its graph, the upstream, the config) and writes to a repository only to switch branch or make a new one. Commit, stage, push, pull, fetch, merge, rebase, setting an upstream and editing the config go to the user's own tool through a visible handoff. A file manager that half-does a git client's job feels broken next to a real one, and each write brings conflicts, credentials, hooks and signing with it; showing state answers what a file manager is asked anyway: where am I, what changed, how far behind. Subversion and CVS are shown from the working copy's own records only, with history fetched only on request and update run in a terminal; TFVC is not supported.

Decided by the owner on 2026-10-16.
