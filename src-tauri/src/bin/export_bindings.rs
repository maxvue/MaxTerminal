//! Utilitário de linha de comando para geração explícita de `src/types/bindings.ts`.

fn main() {
    if let Err(err) = maxterminal_lib::specta::export_bindings() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
