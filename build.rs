fn main() -> Result<(), Box<dyn std::error::Error>> {
    mod cli {
        include!("src/cli.rs");
    }
    use clap::CommandFactory;
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let man_dir = out_dir.join("man1");
    std::fs::create_dir_all(&man_dir)?;
    clap_mangen::generate_to(cli::Cli::command(), &man_dir)?;
    println!("cargo:rerun-if-changed=src/cli.rs");
    Ok(())
}
