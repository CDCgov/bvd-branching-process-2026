# Experiment configuration
A config file is a dictionary of objects referenced during calibration and simulation of the `VHFModel`. It has some required components and some optional components that allow for easier user-specified flexibility. In order to run the detection workflow, it must be

## Required parameters
```json
{
    "priors_file": "experiments/detection/priors.json", // The priors used to propose parameters for calibration
    "target_data_file": {
        "confirmation": "daily_confirmation_incidence.csv", // The confirmation incidence of the outbreak at the national level - copied into `DATA_INPUT_DIR`
        "deaths_upper_threshold": "threshold_data_500.csv", // 500 is the upper bound for confirmed deaths as of intervention start - stored in `DATA_INPUT_DIR`
        "deaths_lower_threshold": "threshold_data_50.csv" // 50 is the lower bound for confirmed deaths as of interventions start - stored in `DATA_INPUT_DIR`
    },
    "strategy": "detection_band", //The detection workflow strategy
    "default_ixa_file": "experiments/detection/default_ixa_config.json", //Any file containing the default ixa parameters
    "exe_file": "target/release/vhf_model", // Consistent path to compiled binaries
    "force_overwrite": true, // Run the workflow whenever this config is called
    "calibration": {
        "generation_particle_count": 2500,
        "tolerance_values": [5.0], // This value can be any float, because 0.0 is the only successful distance
        "entropy": 188178541513900933870725919196083449244, // This int can be any value and determines stochastic behavior
        "clean": true, // whether to remove the simulation files generated during calibration - set to true for storage optimaization
        "keep_previous_population_data": true, // Track the accepted particles, always true because we run ABC, not ABC-SMC rejection sampling
    },
    "projection": {
        "clean": false // Ensure all simulations are kept from the projection phase of the workflow
    }
}

```
