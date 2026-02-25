fn main() {
    let result = specgate::cli::run(std::env::args_os());

    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }

    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    std::process::exit(result.exit_code);
}
