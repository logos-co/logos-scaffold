use crate::commands::build::cmd_build_shortcut;
use crate::commands::deploy::cmd_deploy;
use crate::commands::doctor::cmd_doctor;
use crate::commands::interact::cmd_interact;
use crate::commands::localnet::cmd_localnet;
use crate::commands::new::cmd_new;
use crate::commands::setup::cmd_setup;
use crate::commands::slice::cmd_slice;
use crate::commands::verify::cmd_verify;
use crate::commands::wallet::cmd_wallet;
use crate::constants::VERSION;
use crate::DynResult;

pub(crate) fn run(args: Vec<String>) -> DynResult<()> {
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "create" => cmd_new(&args[2..]),
        "new" => cmd_new(&args[2..]),
        "setup" => cmd_setup(&args[2..]),
        "build" => cmd_build_shortcut(&args[2..]),
        "localnet" => cmd_localnet(&args[2..]),
        "wallet" => cmd_wallet(&args[2..]),
        "deploy" => cmd_deploy(&args[2..]),
        "interact" => cmd_interact(&args[2..]),
        "verify" => cmd_verify(&args[2..]),
        "slice" => cmd_slice(&args[2..]),
        "doctor" => cmd_doctor(),
        "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(())
        }
        other => {
            if let Some(suggested) = suggest_command(other) {
                Err(format!("unknown command: {other}. Did you mean `{suggested}`?").into())
            } else {
                Err(format!("unknown command: {other}").into())
            }
        }
    }
}

pub(crate) fn print_help() {
    println!("logos-scaffold {VERSION}");
    println!("commands:");
    println!(
        "  logos-scaffold create <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH] [--bootstrap]"
    );
    println!(
        "  logos-scaffold new <name> [--vendor-deps] [--lssa-path PATH] [--cache-root PATH] [--bootstrap]"
    );
    println!("  logos-scaffold build [project-path]");
    println!("  logos-scaffold setup");
    println!("  logos-scaffold localnet start");
    println!("  logos-scaffold localnet stop");
    println!("  logos-scaffold localnet status");
    println!("  logos-scaffold localnet logs [--tail N]");
    println!("  logos-scaffold localnet reset");
    println!("  logos-scaffold wallet init");
    println!("  logos-scaffold wallet topup --to <Public/...>");
    println!("  logos-scaffold deploy hello-world");
    println!("  logos-scaffold interact hello-world --account-id <Public/...>");
    println!("  logos-scaffold verify hello-world --account-id <Public/...>");
    println!("  logos-scaffold slice run [--repeat N]");
    println!("  logos-scaffold doctor");
}

pub(crate) fn suggest_command(cmd: &str) -> Option<&'static str> {
    let known = [
        "create", "new", "build", "setup", "localnet", "wallet", "deploy", "interact", "verify",
        "slice", "doctor", "help",
    ];
    let mut best: Option<(&str, usize)> = None;
    for candidate in known {
        let dist = edit_distance(cmd, candidate);
        match best {
            Some((_, best_dist)) if dist >= best_dist => {}
            _ => best = Some((candidate, dist)),
        }
    }

    match best {
        Some((candidate, dist)) if dist <= 4 => Some(candidate),
        _ => None,
    }
}

pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b.len()]
}
