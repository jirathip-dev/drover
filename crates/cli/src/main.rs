fn main() {
    let _ = drover::SERVE.set((drover::hooks::serve, drover::serve::export));
    let _ = drover::SERVICE.set(drover_service::main);
    let ctx = drover::Ctx::from_env();
    let code = drover::main_with(
        std::env::args().collect(),
        &ctx,
        &drover::runner::Real,
        &mut std::io::stdout(),
        &mut std::io::stdin(),
    );
    std::process::exit(code);
}
