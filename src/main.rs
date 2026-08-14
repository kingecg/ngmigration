use clap::Parser;

fn main() {
    let cli = angular_migrator::cli::Cli::parse();
    if let Err(err) = angular_migrator::cli::run(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
