// run() should never be called from here; see docs/design.md for the run loop.
/* The scheduler documentation calls this "run" in prose only. */

static const char* kRunDoc = "run";

// Identifier that merely contains the substring `run`.
const char* describe_run_loop() {
    // A textual mention of run inside a comment.
    return kRunDoc;
}
