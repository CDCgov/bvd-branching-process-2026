import datetime as dt
import json
from pathlib import Path


def load_intervention_date(config_file: Path) -> dt.date:
    """Extract the intervention trigger date from the config chain."""
    with open(config_file, "r") as fp:
        config = json.load(fp)
    with open(config["default_ixa_file"], "r") as fp:
        default_ixa = json.load(fp)
    ixa_params = default_ixa["evdmodel.Parameters"]
    date_str = ixa_params["surveillance_campaign_delay"]["trigger"]["Date"]["date"]
    return dt.date.fromisoformat(date_str)


def load_simulation_start_date(config_file: Path) -> dt.date:
    """Extract simulation start date from the default ixa config chain."""
    with open(config_file, "r") as fp:
        config = json.load(fp)
    with open(config["default_ixa_file"], "r") as fp:
        default_ixa = json.load(fp)
    start_str = default_ixa["evdmodel.Parameters"]["initialization"]["start_date"]
    return dt.date.fromisoformat(start_str)
