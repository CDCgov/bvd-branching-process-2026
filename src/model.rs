use crate::branching_process;
use crate::case_confirmation;
use crate::clinical_manager;
use crate::detection_manager;
use crate::disease_manager;
use crate::infection_initialization;
use crate::offspring_distribution;
use crate::reports;
use crate::shutdown::{plan_shutdown, EndRunConditions};
use crate::transmission_manager;
use crate::vaccination;
use anyhow::Result;
use ixa::{Context, ContextRandomExt};

pub fn load_init_fns(context: &mut Context) -> Result<()> {
    // Initialize the within crate modules
    reports::init(context)?;
    disease_manager::init(context);
    branching_process::init(context);
    offspring_distribution::init(context);
    detection_manager::init(context)?;
    infection_initialization::init(context)?;
    clinical_manager::init(context);
    case_confirmation::init(context)?;
    vaccination::init(context)?;
    transmission_manager::init(context);
    Ok(())
}

pub fn initialize_model(
    context: &mut Context,
    seed: u64,
    end_run_conditions: EndRunConditions,
) -> Result<()> {
    // Initialize the random number generator with the provided seed
    context.init_random(seed);

    // Initialize the within crate modules
    load_init_fns(context)?;
    plan_shutdown(context, end_run_conditions)?;

    Ok(())
}
