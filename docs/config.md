# Experiment configuration
A config file is a dicitonary of objects referenced during calibration and simulation of the `VHFModel`. It has some required components and some optional components that allow for easier user-specified flexibility

## Required parameters


{
    "priors_file": "experiments/bvd_early_phase/priors.json",
    "target_data_file": "threshold_data.csv",
    "strategy": "bvd_threshold",
    "default_ixa_file": "experiments/bvd_early_phase/default_params.json",
    "exe_file": "target/release/vhf_model",
    "force_overwrite": true,
    "outputs_to_read": [
        {
            "spec": "relative",
            "name": "prevalence_report"
        },
        {
            "spec": "relative",
            "name": "symptom_onset_report"
        }
    ],
    "calibration": {
        "generation_particle_count": 500,
        "tolerance_values": [0],
        "entropy": 188178541513900933870725919196083449244,
        "clean": true
    },
    "projection": {
        "clean": false,
        "ixa_overrides": {
            "max_time": 300.0,
            "max_population": 30000
        }
    }
}
```
