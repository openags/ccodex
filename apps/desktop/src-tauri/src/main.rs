mod commands;
mod shell;

fn main() {
    println!(
        "{}: {}",
        shell::shell_name(),
        commands::local_server_banner()
    );
}
