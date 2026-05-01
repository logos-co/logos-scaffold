use std::collections::HashMap;

fn main() {
    let docker_options = risc0_build::DockerOptionsBuilder::default()
        .root_dir("..")
        .build()
        .expect("docker options should build");
    let guest_options = risc0_build::GuestOptionsBuilder::default()
        .use_docker(docker_options)
        .build()
        .expect("guest options should build");
    let mut options = HashMap::new();
    options.insert("example_program_deployment_programs", guest_options);
    risc0_build::embed_methods_with_options(options);
}
