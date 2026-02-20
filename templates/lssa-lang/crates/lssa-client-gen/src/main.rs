use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut idl_dir: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--idl-dir" => {
                let value = args.get(i + 1).ok_or("--idl-dir requires value")?;
                idl_dir = Some(PathBuf::from(value));
                i += 2;
            }
            "--out-dir" => {
                let value = args.get(i + 1).ok_or("--out-dir requires value")?;
                out_dir = Some(PathBuf::from(value));
                i += 2;
            }
            other => {
                return Err(format!("unknown argument: {other}").into());
            }
        }
    }

    let idl_dir = idl_dir.ok_or("missing --idl-dir")?;
    let out_dir = out_dir.ok_or("missing --out-dir")?;

    lssa_client_gen::generate_clients(&idl_dir, &out_dir)
}
