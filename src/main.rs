fn main() {
    if let Err(e) = forge_viewer::run() {
        eprintln!("viewer error: {e}");
    }
}
