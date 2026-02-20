#[allow(dead_code)]
pub mod generated;

#[allow(dead_code)]
pub mod runner_support {
    use nssa::{AccountId, program::Program};

    pub fn parse_account_id(raw: &str) -> AccountId {
        let normalized = raw
            .strip_prefix("Public/")
            .or_else(|| raw.strip_prefix("Private/"))
            .unwrap_or(raw);

        normalized
            .parse()
            .unwrap_or_else(|err| panic!("invalid account_id `{raw}`: {err}"))
    }

    pub fn load_program(program_path: Option<&str>, embedded_elf: &[u8], label: &str) -> Program {
        let bytes = if let Some(path) = program_path {
            std::fs::read(path)
                .unwrap_or_else(|err| panic!("failed to read {label} binary at `{path}`: {err}"))
        } else {
            embedded_elf.to_vec()
        };

        Program::new(bytes).unwrap_or_else(|err| panic!("failed to parse {label} program: {err}"))
    }
}
