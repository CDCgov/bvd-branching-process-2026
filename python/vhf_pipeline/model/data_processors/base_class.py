from abc import ABC, abstractmethod
from pathlib import Path

import polars as pl


class NaturalHistoryProcessor(ABC):
    # ixa report param names this objective consumes. During calibration, every
    # other report's `write` is turned off so particles don't spend time writing
    # and reading CSVs the distance function never looks at.
    required_reports: tuple[str, ...] = ()

    @abstractmethod
    def estimate_error(
        self, outputs: dict[str, pl.DataFrame], target_df: pl.DataFrame
    ) -> float:
        pass

    @abstractmethod
    def get_target_data(
        self, target_data_file: str | Path | dict[str, Path]
    ) -> pl.DataFrame:
        pass

    @abstractmethod
    def process_outputs(self, outputs: dict[str, pl.DataFrame]) -> pl.DataFrame:
        pass
