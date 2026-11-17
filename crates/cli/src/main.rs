fn main() {
    drover::SERVE.set((drover::hooks::serve, drover::serve::export)).ok();
    drover::SERVICE.set(drover_service::main).ok();
    let ctx = drover::Ctx::from_env();
    let code = drover::main_with(
        std::env::args().collect(),
        &ctx,
        &drover::runner::Real,
        &mut std::io::stdout(),
        &mut std::io::stdin(),
    );
    drover_core::runner::exit(code);
}
