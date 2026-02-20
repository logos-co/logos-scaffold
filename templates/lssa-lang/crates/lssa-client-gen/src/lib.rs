use std::fs;
use std::path::{Path, PathBuf};

use lssa_idl_spec::ProgramIdl;

pub fn generate_clients(idl_dir: &Path, out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(out_dir)?;

    let mut files: Vec<PathBuf> = fs::read_dir(idl_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();

    if files.is_empty() {
        return Err(format!("no idl json files found in {}", idl_dir.display()).into());
    }

    let mut module_names = Vec::new();
    for path in files {
        let raw = fs::read_to_string(&path)?;
        let idl: ProgramIdl = serde_json::from_str(&raw)?;
        let module_name = format!("{}_client", snake_case(&idl.program.name));
        let code = generate_client_source(&idl);
        fs::write(out_dir.join(format!("{module_name}.rs")), code)?;
        module_names.push(module_name);
    }

    module_names.sort();
    module_names.dedup();

    let mut mod_rs = String::new();
    for module in module_names {
        mod_rs.push_str(&format!("pub mod {module};\n"));
    }
    fs::write(out_dir.join("mod.rs"), mod_rs)?;

    Ok(())
}

fn generate_client_source(idl: &ProgramIdl) -> String {
    let program_pascal = pascal_case(&idl.program.name);
    let instruction_enum = format!("{program_pascal}Instruction");
    let client_name = format!("{program_pascal}Client");

    let mut out = String::new();
    out.push_str("use common::rpc_primitives::requests::SendTxResponse;\n");
    out.push_str("use nssa::{\n");
    out.push_str("    AccountId, PrivateKey, PublicTransaction,\n");
    out.push_str("    privacy_preserving_transaction::circuit::ProgramWithDependencies,\n");
    out.push_str("    program::Program,\n");
    out.push_str("    public_transaction::{Message, WitnessSet},\n");
    out.push_str("};\n");
    out.push_str("use nssa_core::SharedSecretKey;\n");
    out.push_str("use wallet::{PrivacyPreservingAccount, WalletCore};\n\n");

    out.push_str("#[derive(Clone, Copy, Debug)]\n");
    out.push_str("pub enum AccountRef {\n");
    out.push_str("    Public(AccountId),\n");
    out.push_str("    PrivateOwned(AccountId),\n");
    out.push_str("}\n\n");

    out.push_str("impl AccountRef {\n");
    out.push_str("    fn account_id(self) -> AccountId {\n");
    out.push_str("        match self {\n");
    out.push_str("            AccountRef::Public(id) | AccountRef::PrivateOwned(id) => id,\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
    out.push_str("    fn is_private_owned(self) -> bool {\n");
    out.push_str("        matches!(self, AccountRef::PrivateOwned(_))\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("pub enum LssaSubmitResult {\n");
    out.push_str("    Public(SendTxResponse),\n");
    out.push_str("    PrivateOwned {\n");
    out.push_str("        response: SendTxResponse,\n");
    out.push_str("        shared_secrets: Vec<SharedSecretKey>,\n");
    out.push_str("    },\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Clone, Debug, serde::Serialize)]\n");
    out.push_str(&format!("enum {instruction_enum} {{\n"));
    for instruction in &idl.instructions {
        if instruction.args.is_empty() {
            out.push_str(&format!("    {},\n", instruction.variant));
        } else {
            out.push_str(&format!("    {} {{\n", instruction.variant));
            for arg in &instruction.args {
                let arg_name = rust_ident(&arg.name);
                let ty = rust_type(&arg.ty);
                out.push_str(&format!("        {arg_name}: {ty},\n"));
            }
            out.push_str("    },\n");
        }
    }
    out.push_str("}\n\n");

    for instruction in &idl.instructions {
        let accounts_name = format!("{}Accounts", pascal_case(&instruction.name));
        out.push_str(&format!("pub struct {accounts_name} {{\n"));
        for account in &instruction.accounts {
            let field_name = rust_ident(&account.name);
            out.push_str(&format!("    pub {field_name}: AccountRef,\n"));
        }
        out.push_str("}\n\n");
    }

    out.push_str("struct AccountInput {\n");
    out.push_str("    name: &'static str,\n");
    out.push_str("    account_ref: AccountRef,\n");
    out.push_str("    auth: bool,\n");
    out.push_str("}\n\n");

    out.push_str(&format!("pub struct {client_name}<'w> {{\n"));
    out.push_str("    wallet_core: &'w WalletCore,\n");
    out.push_str("    program: Program,\n");
    out.push_str("}\n\n");

    out.push_str(&format!("impl<'w> {client_name}<'w> {{\n"));
    out.push_str("    pub fn new(wallet_core: &'w WalletCore, program: Program) -> Self {\n");
    out.push_str("        Self { wallet_core, program }\n");
    out.push_str("    }\n\n");

    for instruction in &idl.instructions {
        let method_name = rust_ident(&snake_case(&instruction.name));
        let accounts_name = format!("{}Accounts", pascal_case(&instruction.name));

        let mut method_signature = format!(
            "    pub async fn {method_name}(\n        &self,\n        accounts: {accounts_name}",
        );
        for arg in &instruction.args {
            method_signature.push_str(&format!(",\n        {}: {}", rust_ident(&arg.name), rust_type(&arg.ty)));
        }
        method_signature.push_str(
            ",\n    ) -> Result<LssaSubmitResult, Box<dyn std::error::Error>> {\n",
        );
        out.push_str(&method_signature);

        if instruction.args.is_empty() {
            out.push_str(&format!(
                "        let instruction = {instruction_enum}::{};\n",
                instruction.variant
            ));
        } else {
            out.push_str(&format!(
                "        let instruction = {instruction_enum}::{} {{\n",
                instruction.variant
            ));
            for arg in &instruction.args {
                let arg_name = rust_ident(&arg.name);
                out.push_str(&format!("            {arg_name},\n"));
            }
            out.push_str("        };\n");
        }

        out.push_str("        self.submit(instruction, vec![\n");
        for account in &instruction.accounts {
            let field_name = rust_ident(&account.name);
            out.push_str(&format!(
                "            AccountInput {{ name: \"{}\", account_ref: accounts.{field_name}, auth: {} }},\n",
                account.name,
                account.auth
            ));
        }
        out.push_str("        ]).await\n");
        out.push_str("    }\n\n");
    }

    out.push_str(&format!(
        "    async fn submit(\n        &self,\n        instruction: {instruction_enum},\n        account_inputs: Vec<AccountInput>,\n    ) -> Result<LssaSubmitResult, Box<dyn std::error::Error>> {{\n"
    ));
    out.push_str("        let has_private = account_inputs.iter().any(|input| input.account_ref.is_private_owned());\n\n");
    out.push_str("        if has_private {\n");
    out.push_str("            let privacy_accounts = account_inputs\n");
    out.push_str("                .iter()\n");
    out.push_str("                .map(|input| match input.account_ref {\n");
    out.push_str("                    AccountRef::Public(id) => PrivacyPreservingAccount::Public(id),\n");
    out.push_str("                    AccountRef::PrivateOwned(id) => PrivacyPreservingAccount::PrivateOwned(id),\n");
    out.push_str("                })\n");
    out.push_str("                .collect::<Vec<_>>();\n\n");
    out.push_str("            let instruction_data = Program::serialize_instruction(instruction)?;\n");
    out.push_str("            let program_with_dependencies = ProgramWithDependencies::from(self.program.clone());\n");
    out.push_str("            let (response, shared_secrets) = self\n");
    out.push_str("                .wallet_core\n");
    out.push_str("                .send_privacy_preserving_tx(privacy_accounts, instruction_data, &program_with_dependencies)\n");
    out.push_str("                .await?;\n\n");
    out.push_str("            return Ok(LssaSubmitResult::PrivateOwned { response, shared_secrets });\n");
    out.push_str("        }\n\n");

    out.push_str("        let account_ids = account_inputs\n");
    out.push_str("            .iter()\n");
    out.push_str("            .map(|input| input.account_ref.account_id())\n");
    out.push_str("            .collect::<Vec<_>>();\n\n");

    out.push_str("        let auth_public_accounts = account_inputs\n");
    out.push_str("            .iter()\n");
    out.push_str("            .filter_map(|input| {\n");
    out.push_str("                if !input.auth {\n");
    out.push_str("                    return None;\n");
    out.push_str("                }\n");
    out.push_str("                match input.account_ref {\n");
    out.push_str("                    AccountRef::Public(id) => Some((input.name, id)),\n");
    out.push_str("                    AccountRef::PrivateOwned(_) => None,\n");
    out.push_str("                }\n");
    out.push_str("            })\n");
    out.push_str("            .collect::<Vec<_>>();\n\n");

    out.push_str("        let auth_account_ids = auth_public_accounts.iter().map(|(_, id)| *id).collect::<Vec<_>>();\n\n");

    out.push_str("        let nonces = if auth_account_ids.is_empty() {\n");
    out.push_str("            Vec::new()\n");
    out.push_str("        } else {\n");
    out.push_str("            self.wallet_core.get_accounts_nonces(auth_account_ids).await?\n");
    out.push_str("        };\n\n");

    out.push_str("        let mut signing_keys: Vec<&PrivateKey> = Vec::with_capacity(auth_public_accounts.len());\n");
    out.push_str("        for (name, account_id) in &auth_public_accounts {\n");
    out.push_str("            let signing_key = self\n");
    out.push_str("                .wallet_core\n");
    out.push_str("                .storage()\n");
    out.push_str("                .user_data\n");
    out.push_str("                .get_pub_account_signing_key(*account_id)\n");
    out.push_str("                .ok_or_else(|| std::io::Error::other(format!(\"missing public signing key for `{name}` account {account_id}\")))?;\n");
    out.push_str("            signing_keys.push(signing_key);\n");
    out.push_str("        }\n\n");

    out.push_str("        let message = Message::try_new(self.program.id(), account_ids, nonces, instruction)?;\n");
    out.push_str("        let witness_set = WitnessSet::for_message(&message, &signing_keys);\n");
    out.push_str("        let tx = PublicTransaction::new(message, witness_set);\n");
    out.push_str("        let response = self.wallet_core.sequencer_client.send_tx_public(tx).await?;\n\n");
    out.push_str("        Ok(LssaSubmitResult::Public(response))\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

fn rust_type(ty: &str) -> String {
    let normalized = ty.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
    match normalized.as_str() {
        "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64"
        | "i128" | "bool" | "String" => normalized,
        "Vec<u8>" => "Vec<u8>".to_string(),
        other => other.to_string(),
    }
}

fn snake_case(raw: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in raw.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    collapse_separators(&out, '_')
}

fn pascal_case(raw: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper {
                out.push(ch.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(ch.to_ascii_lowercase());
            }
        } else {
            upper = true;
        }
    }
    if out.is_empty() {
        "Program".to_string()
    } else {
        out
    }
}

fn rust_ident(raw: &str) -> String {
    let ident = collapse_separators(
        &raw.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>(),
        '_',
    );

    match ident.as_str() {
        "type" | "match" | "mod" | "enum" | "struct" | "fn" | "crate" => {
            format!("r#{ident}")
        }
        _ => ident,
    }
}

fn collapse_separators(raw: &str, sep: char) -> String {
    let mut out = String::new();
    let mut prev_sep = false;

    for ch in raw.chars() {
        if ch == sep {
            if !prev_sep {
                out.push(sep);
                prev_sep = true;
            }
        } else {
            out.push(ch);
            prev_sep = false;
        }
    }

    let out = out.trim_matches(sep).to_string();
    if out.is_empty() {
        "value".to_string()
    } else {
        out
    }
}
