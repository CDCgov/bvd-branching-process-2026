import json
import shutil
import urllib.parse
import urllib.request
from pathlib import Path

import polars as pl

from vhf_pipeline import paths


def get_foundry_data(host, rid, output: Path, verbose: bool) -> None:
    base = f"https://{host}/api/v2/datasets/{rid}"
    headers = {"Authorization": f"Bearer {paths.load_foundry_token()}"}

    def get(path):
        return urllib.request.urlopen(
            urllib.request.Request(base + path, headers=headers)
        )

    output.mkdir(parents=True, exist_ok=True)
    assert output.exists() and output.is_dir()

    with get(path="/files?branchName=master") as response:
        files = json.load(response)["data"]

    for file in files:
        name = Path(file["path"]).name
        path = urllib.parse.quote(file["path"], safe="")
        output_path = output / name

        with (
            get(path=f"/files/{path}/content?branchName=master") as response,
            open(output_path, "wb") as target,
        ):
            shutil.copyfileobj(response, target)

        if verbose:
            print(output / name)


def save_foundry_input(filename: str, rid_override: str, verbose: bool = False) -> None:
    host, rid = paths.load_foundry_rid(override=rid_override)
    tmp_dir = paths.data_input_dir() / ".tmp"
    get_foundry_data(host=host, rid=rid, output=tmp_dir, verbose=verbose)
    pl.read_parquet(tmp_dir / "*.parquet").write_csv(paths.data_input_dir() / filename)

    # Clean up the temporary directory after loading the data
    shutil.rmtree(tmp_dir)


if __name__ == "__main__":
    host, rid = paths.load_foundry_rid()
    get_foundry_data(host=host, rid=rid, output=Path("data/loaded/"), verbose=True)
    pl.read_parquet("data/loaded/*.parquet").write_csv("data/linelist.csv")
