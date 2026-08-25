import json
from pathlib import Path

import griddler
import polars as pl


def read_griddle(griddle_obj: dict | str | Path) -> pl.DataFrame:
    # Load the parameter sets
    if isinstance(griddle_obj, (str, Path)):
        input_griddle = griddle_obj
        with open(input_griddle, "r") as fp:
            raw_griddle = json.load(fp)
    elif isinstance(griddle_obj, dict):
        raw_griddle = griddle_obj

    griddle = griddler.parse(raw_griddle)
    par_sets = [spec for spec in iter(griddle.specs)]
    par_sets = pl.DataFrame(par_sets, strict=False)

    return par_sets
