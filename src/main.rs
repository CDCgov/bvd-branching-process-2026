use evdmodel::{initialize_model, ContextParametersExt, ParameterValues};
use ixa::{run_with_args, Context};

fn main() {
    run_with_args(|context: &mut Context, args, _| {
        assert!(
            args.config.is_some(),
            "No config file provided, must follow `cargo run -- --config input/input.json`"
        );

        let &ParameterValues {
            seed,
            end_run_conditions,
            ..
        } = context.get_params();
        initialize_model(context, seed, end_run_conditions).expect("Model run failed");

        Ok(())
    })
    .unwrap();
}
