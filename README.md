# CFA EVD 2025 Interventions

## Overview

This repo contains a branching process model developed during and immediately after the fall 2025 viral hemorrhagic fever response. Primarily, this codebase serves as a streamlined version of the model built for the Ebolavirus disease (EVD) outbreak in the Democratic Republic of the Congo.

## Getting started

This repo uses uv for dependency management, be sure that uv is [installed on your machine](https://docs.astral.sh/uv/getting-started/installation/).

To use this model, you need to have Rust and Cargo installed. You can find instructions for installing Rust [here](https://www.rust-lang.org/tools/install).

The examples currently available are as follows:

1. Build the rust binaries using `mise build` or
```
uv run cargo build -r
```

2. Run the model once using `mise run vhf_model` or
```
uv run vhf model run
```

Each workflow run writes a `manifest.json` at the root of its output directory recording how the run was produced: the git commit/branch (and whether the tree was dirty), the `vhf_pipeline`/Python/key dependency versions, the resolved command and arguments, and the SHA-256 of the config and griddle it ran with — enough to trace a result back to the code, environment, and inputs that made it.

## Repository layout

The Rust model lives in `src/` (crate `vhf_model`, binary `target/release/vhf_model`). The Python lives under `python/vhf_pipeline/`, kept separate from `src/` so the two source roots never collide:

- `vhf_pipeline/model/` — bindings to the ixa model: the `VHFModel` runner (shells out to the compiled binary today), calibration/projection contexts, and output handlers. This is the single seam between Python and Rust.
- `vhf_pipeline/pipeline/` — reusable pipeline stages (`calibrate`, `analytics`, `binning`, `figures`), each runnable on its own.
- `vhf_pipeline/workflows/` — named end-to-end workflows that compose the stages in order: `detection` runs the full threshold and detection range workstream. `vhf workflow <name>` dispatches by name, so adding a workflow is just a new module here.
- `vhf_pipeline/cli/` — the `vhf` entry point that drives all of the above: `run_model.py` builds the `vhf model` group (`run`, `calibrate`) and `run_workflow.py` builds the `vhf workflow` group, assembled in `__init__.py`.
- `vhf_pipeline/provenance.py` — builds and writes the per-run `manifest.json` (stdlib-only). Workflows opt in by overriding `Workflow.manifest_dir`; the base class writes the manifest before the run starts.

## Foundry data

1. Get a Foundry token: Go to 1CDP, account, create a token
2. Copy `.env.example` to `.env`. Fill in the Token and make an RID tag under `data/rid_tags.json`.
3. Run `get_foundry_data.py` or include the `main` function in your pipeline.

## Project admins

- Will Koval <{{ad71@cdc.gov}}> (CDC/IOD/ORR/CFA)
- Eric Mooring <{{pgv5@cdc.gov}}> (CDDC/IOD/ORR/CFA)
- Guido Camargo España, <{{ukd0@cdc.gov}}> (CDC/OD/ORR/CFA)

## Disclaimers

### General Disclaimer

This repository was created for use by CDC programs to collaborate on public health related projects in support of the [CDC mission](https://www.cdc.gov/about/organization/mission.htm). GitHub is not hosted by the CDC, but is a third party website used by CDC and its partners to share information and collaborate on software. CDC use of GitHub does not imply an endorsement of any one particular service, product, or enterprise.

### Public Domain Standard Notice

This repository constitutes a work of the United States Government and is not
subject to domestic copyright protection under 17 USC § 105. This repository is in
the public domain within the United States, and copyright and related rights in
the work worldwide are waived through the [CC0 1.0 Universal public domain dedication](https://creativecommons.org/publicdomain/zero/1.0/).
All contributions to this repository will be released under the CC0 dedication. By
submitting a pull request you are agreeing to comply with this waiver of
copyright interest.

### License Standard Notice

This repository is licensed under Apache-2.0 or later.

This source code in this repository is free: you can redistribute it and/or modify it under
the terms of the Apache License, Version 2.0, or (at your option) any
later version.

This source code in this repository is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE. See the Apache Software License for more details.

You should have received a copy of the Apache Software License along with this
program. If not, see http://www.apache.org/licenses/LICENSE-2.0.html

The source code forked from other open source projects will inherit its license.

### Privacy Standard Notice

This repository contains only non-sensitive, publicly available data and
information. All material and community participation is covered by the
[Disclaimer](https://github.com/CDCgov/template/blob/master/DISCLAIMER.md)
and [Code of Conduct](https://github.com/CDCgov/template/blob/master/code-of-conduct.md).
For more information about CDC's privacy policy, please visit [http://www.cdc.gov/other/privacy.html](https://www.cdc.gov/other/privacy.html).

### Contributing Standard Notice

Anyone is encouraged to contribute to the repository by [forking](https://help.github.com/articles/fork-a-repo)
and submitting a pull request. (If you are new to GitHub, you might start with a
[basic tutorial](https://help.github.com/articles/set-up-git).) By contributing
to this project, you grant a world-wide, royalty-free, perpetual, irrevocable,
non-exclusive, transferable license to all users under the terms of the
[Apache Software License v2](http://www.apache.org/licenses/LICENSE-2.0.html) or
later.

All comments, messages, pull requests, and other submissions received through
CDC including this GitHub page may be subject to applicable federal law, including but not limited to the Federal Records Act, and may be archived. Learn more at [http://www.cdc.gov/other/privacy.html](http://www.cdc.gov/other/privacy.html).

### Records Management Standard Notice

This repository is not a source of government records but is a copy to increase
collaboration and collaborative potential. All government records will be
published through the [CDC web site](http://www.cdc.gov).
