use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgramIdl {
    pub spec: String,
    pub metadata: Metadata,
    pub program: Program,
    pub instructions: Vec<Instruction>,
    #[serde(default)]
    pub types: Vec<TypeDef>,
    #[serde(default)]
    pub errors: Vec<ErrorDef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Program {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Instruction {
    pub name: String,
    pub variant: String,
    pub discriminator: Vec<u8>,
    pub args: Vec<InstructionArg>,
    pub accounts: Vec<AccountField>,
    pub execution: Execution,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstructionArg {
    pub name: String,
    pub ty: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountField {
    pub name: String,
    pub ty: String,
    #[serde(default)]
    pub mutable: bool,
    #[serde(default)]
    pub auth: bool,
    #[serde(default)]
    pub claim_if_default: bool,
    #[serde(default)]
    pub visibility: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Execution {
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub private_owned: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeDef {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorDef {
    pub code: u32,
    pub name: String,
}
