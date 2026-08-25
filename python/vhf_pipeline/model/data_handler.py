from pathlib import Path

import polars as pl

from .data_processors import NaturalHistoryProcessor, init_processor


class NaturalHistoryHandler:
    def __init__(self, strategy: str):
        self.strategy = strategy
        self.processor = self._get_processor(self.strategy)

    def _get_processor(
        self,
        strategy: str,
    ) -> NaturalHistoryProcessor:
        return init_processor(strategy)

    @property
    def required_reports(self) -> tuple[str, ...]:
        return self.processor.required_reports

    def estimate_error(
        self, outputs: dict[str, pl.DataFrame], target_df: pl.DataFrame
    ) -> float:
        return self.processor.estimate_error(outputs, target_df)

    def get_target_data(
        self, target_data_file: str | Path | dict[str, Path]
    ) -> pl.DataFrame:
        return self.processor.get_target_data(target_data_file)

    def process_outputs(self, outputs: dict[str, pl.DataFrame]) -> pl.DataFrame:
        return self.processor.process_outputs(outputs)
