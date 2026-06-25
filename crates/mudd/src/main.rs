use std::io::Write;

/// Placeholder entry point for the `mudd` server binary.
///
/// Real boot wiring (world load, DB pool, scheduler, gateway) arrives in
/// M1-22. Until then this only proves the workspace builds and runs.
fn main() -> std::io::Result<()> {
    std::io::stdout().write_all(b"ferrodun mudd placeholder\n")
}
