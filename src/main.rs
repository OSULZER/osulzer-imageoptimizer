// Kein Konsolenfenster beim Start aus dem Explorer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use osulzer_imageoptimizer::cli::{CLI_COMMANDS, Command, parse_args};
use osulzer_imageoptimizer::cli_run;
use osulzer_imageoptimizer::instance::{self, Role};
use osulzer_imageoptimizer::i18n::t;
use osulzer_imageoptimizer::{config, migrate, shell, ui};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // Kommandozeile: an das aufrufende Terminal hängen, bevor etwas ausgegeben wird.
    if args.first().and_then(|a| a.to_str()).is_some_and(|a| CLI_COMMANDS.contains(&a)) {
        cli_run::attach_parent_console();
    }
    osulzer_imageoptimizer::i18n::set_lang(config::load_from(&config::config_file()).ui_lang());
    let paths = match parse_args(args) {
        Ok(Command::Register) => return exit_code(std::env::current_exe().and_then(|exe| shell::register(&exe, t("menu.label")))),
        Ok(Command::Unregister) => return exit_code(shell::unregister()),
        Ok(Command::Open(paths)) => paths,
        Ok(Command::Cli(cli)) => return ExitCode::from(cli_run::run(cli, &mut std::io::stdout(), &mut std::io::stderr())),
        // Hilfe und Version sind keine Fehler.
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { cli_run::USAGE } else { cli_run::OK });
        }
    };

    // Je Benutzer eine Instanz; der Name darf nur unverfängliche Zeichen enthalten.
    let user: String = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    // Neustart nach einem Update oder einer Installation: erst übernehmen, wenn die alte Instanz beendet ist.
    osulzer_imageoptimizer::update::wait_for_previous_instance();
    if let Ok(exe) = std::env::current_exe() {
        osulzer_imageoptimizer::update::swap::cleanup_old(&exe);
        osulzer_imageoptimizer::install::after_start(&exe);
    }
    let incoming = match instance::acquire(&config::config_dir(), &format!("osulzer-imageoptimizer-{user}"), &paths) {
        Ok(Role::Secondary) => return ExitCode::SUCCESS,
        Ok(Role::Primary(incoming)) => incoming,
        // Lieber ein zweites Fenster als verlorene Bilder.
        Err(_) => std::sync::mpsc::channel().1,
    };
    // Erst hier: nur die erste Instanz zieht um, weitere Aufrufe reichen nur ihre Dateien weiter.
    let notes = match std::env::current_exe() {
        Ok(exe) => migrate::run(&exe).errors,
        Err(_) => Vec::new(),
    };
    match ui::run_app(paths, incoming, notes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn exit_code(result: std::io::Result<()>) -> ExitCode {
    if result.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
