import datetime as dt
from pathlib import Path

import polars as pl


def write_threshold_data(
    directory: Path, threshold: int, date: dt.date, lag: int | None = None
) -> str:
    """
    Write threshold data to a CSV file.

    Parameters:
    - directory: Path to the output directory
    - threshold: The threshold value
    - date: The date of the threshold
    - lag: Optional lag value (default is None)
    """
    # Create a DataFrame with the threshold data
    data = {
        "threshold": [threshold],
        "threshold_date": [date.isoformat()],
        "lag": [lag] if lag is not None else [""],
    }
    df = pl.DataFrame(data)

    # Write the DataFrame to a CSV file
    filename = f"threshold_data_{threshold}.csv"
    fp = directory / filename
    df.write_csv(fp)
    return filename
