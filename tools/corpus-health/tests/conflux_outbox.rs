// Compile the production audition path for a focused integration test. No library,
// corpus walk, worker pool or CLI entry point is invoked by the selected test.
#[path = "../src/main.rs"]
mod corpus_health;
