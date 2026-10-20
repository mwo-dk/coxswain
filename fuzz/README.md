# Fuzzing the readers

`cargo +nightly fuzz run archive` (or `files`, `text`) feeds random bytes to the readers of
untrusted files: archives listed, looked into and extracted (nothing may land outside the folder
made), pictures, disk image headers, office files, mail, PDF, PGN, dBase, the name index, the
search store, state.json, config.toml and the text readers. `FUZZ_ENDINGS=psd,pcx` keeps a run
on some kinds. Corpora and crashes stay out of the repository; a crash found becomes a test of
the reader it is about.
