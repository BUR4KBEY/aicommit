#[tokio::main]
async fn main() {
    if let Err(error) = aicommit::run().await {
        // `--json` machine flows already emitted a single-line `error` object
        // to stdout and print diagnostics to stderr themselves; every other
        // path funnels through here for one `Error:` line plus taxonomy code.
        if !aicommit::output::json_error_already_emitted(&error) {
            eprintln!("Error: {error:#}");
        }
        std::process::exit(aicommit::exit::code_for(&error));
    }
}
