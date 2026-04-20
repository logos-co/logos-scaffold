fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(err) = logos_scaffold::run(args) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
