pub use lssa_idl_spec as idl;
pub use lssa_lang_macros::{LssaAccounts, LssaInstruction, lssa_program};

#[derive(Clone, Debug)]
pub struct InstructionArgMetadata {
    pub name: String,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct InstructionVariantMetadata {
    pub name: String,
    pub args: Vec<InstructionArgMetadata>,
}

#[derive(Clone, Debug)]
pub struct AccountFieldMetadata {
    pub name: String,
    pub ty: String,
    pub mutable: bool,
    pub auth: bool,
    pub claim_if_default: bool,
    pub visibility: Vec<String>,
}

pub trait LssaInstruction {
    fn lssa_instruction_variants() -> Vec<InstructionVariantMetadata>;
}

pub trait LssaAccounts {
    fn lssa_account_fields() -> Vec<AccountFieldMetadata>;
}

pub struct InstructionRegistration<'a> {
    pub name: &'a str,
    pub instruction_ty: &'a str,
    pub accounts_ty: &'a str,
    pub execution: &'a [&'a str],
}

pub fn build_instruction<I: LssaInstruction, A: LssaAccounts>(
    registration: InstructionRegistration<'_>,
) -> idl::Instruction {
    let variants = I::lssa_instruction_variants();
    let selected = variants
        .iter()
        .find(|variant| normalize_name(&variant.name) == normalize_name(registration.name))
        .cloned()
        .unwrap_or_else(|| {
            variants
                .into_iter()
                .next()
                .unwrap_or_else(|| InstructionVariantMetadata {
                    name: registration.name.to_string(),
                    args: Vec::new(),
                })
        });

    let execution = registration.execution;
    let supports_public = execution.iter().any(|mode| *mode == "public");
    let supports_private_owned = execution.iter().any(|mode| *mode == "private_owned");

    idl::Instruction {
        name: registration.name.to_string(),
        variant: selected.name,
        discriminator: instruction_discriminator(registration.name),
        args: selected
            .args
            .into_iter()
            .map(|arg| idl::InstructionArg {
                name: arg.name,
                ty: arg.ty,
            })
            .collect(),
        accounts: A::lssa_account_fields()
            .into_iter()
            .map(|field| idl::AccountField {
                name: field.name,
                ty: field.ty,
                mutable: field.mutable,
                auth: field.auth,
                claim_if_default: field.claim_if_default,
                visibility: field.visibility,
            })
            .collect(),
        execution: idl::Execution {
            public: supports_public,
            private_owned: supports_private_owned,
        },
    }
}

pub fn build_program_idl(
    spec: &str,
    program_name: &str,
    version: &str,
    instructions: Vec<idl::Instruction>,
) -> idl::ProgramIdl {
    idl::ProgramIdl {
        spec: spec.to_string(),
        metadata: idl::Metadata {
            name: program_name.to_string(),
            version: version.to_string(),
        },
        program: idl::Program {
            name: program_name.to_string(),
        },
        instructions,
        types: Vec::new(),
        errors: Vec::new(),
    }
}

pub fn instruction_discriminator(name: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let preimage = format!("global:{name}");
    let digest = Sha256::digest(preimage.as_bytes());
    digest[0..8].to_vec()
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub mod prelude {
    pub use crate::{
        LssaAccounts, LssaInstruction, lssa_program,
        idl::{AccountField, Execution, Instruction, InstructionArg, ProgramIdl},
    };
}
